use std::{collections::HashMap, sync::Arc, time::Duration};

use anyhow::{Context, Result};
use async_trait::async_trait;
use sqlx::PgPool;
use strife_db::{BackfillCampaignRecord, BackfillKind, BackfillRefillWindow, BackfillState};
use tokio::sync::watch;
use tracing::{debug, error, warn};

/// Kind-specific candidate selection boundary used by the shared coordinator.
///
/// Implementations own the enqueue-and-cursor transaction. Merely registering
/// a provider never starts a draft or paused campaign.
#[async_trait]
pub trait BackfillCandidateProvider: Send + Sync {
    async fn refill(
        &self,
        campaign: &BackfillCampaignRecord,
        window: &BackfillRefillWindow,
    ) -> Result<()>;
}

/// Registry-based low-water scheduler. Feature epics register their adapters.
#[derive(Default)]
pub struct BackfillCoordinator {
    providers: HashMap<BackfillKind, Arc<dyn BackfillCandidateProvider>>,
}

impl BackfillCoordinator {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    #[must_use]
    pub fn with_provider(
        mut self,
        kind: BackfillKind,
        provider: Arc<dyn BackfillCandidateProvider>,
    ) -> Self {
        self.providers.insert(kind, provider);
        self
    }

    /// Runs one bounded refill pass for each explicitly running campaign.
    ///
    /// # Errors
    ///
    /// Returns an error when campaign state cannot be loaded or a registered
    /// provider cannot complete its bounded refill transaction.
    pub async fn run_once(&self, pool: &PgPool) -> Result<()> {
        for campaign in strife_db::list_backfill_campaigns(pool)
            .await
            .context("list backfill campaigns")?
        {
            if campaign.state != BackfillState::Running {
                continue;
            }
            let Some(provider) = self.providers.get(&campaign.kind) else {
                debug!(campaign_id = %campaign.id, kind = ?campaign.kind, "no backfill adapter registered");
                continue;
            };
            let Some(window) = strife_db::get_backfill_refill_window(pool, campaign.id)
                .await
                .context("load backfill refill window")?
            else {
                continue;
            };
            let canary_limit = campaign
                .candidate_definition
                .get("canary_limit")
                .and_then(serde_json::Value::as_i64);
            if window.allowance == 0
                && window.queued == 0
                && canary_limit.is_some_and(|limit| campaign.enqueued_count >= limit)
            {
                strife_db::transition_backfill_campaign(
                    pool,
                    campaign.id,
                    BackfillState::Paused,
                    Some("canary limit reached"),
                )
                .await
                .context("pause completed canary")?;
                continue;
            }
            if window.allowance > 0 {
                provider.refill(&campaign, &window).await?;
            }
        }
        Ok(())
    }
}

/// Historical OCR adapter for the shared coordinator.
///
/// Selection, enqueue, and cursor advance happen inside one database
/// transaction owned by `enqueue_ocr_backfill_batch`. This type only supplies
/// the engine identity that defines which files still need OCR, and marks a
/// campaign drained once its candidate set is exhausted.
pub struct OcrBackfillProvider {
    pool: PgPool,
    supported_mimes: Vec<String>,
}

impl OcrBackfillProvider {
    #[must_use]
    pub fn new(pool: PgPool) -> Self {
        Self {
            pool,
            supported_mimes: strife_media::supported_ocr_mimes()
                .iter()
                .map(|mime| (*mime).to_owned())
                .collect(),
        }
    }
}

#[async_trait]
impl BackfillCandidateProvider for OcrBackfillProvider {
    async fn refill(
        &self,
        campaign: &BackfillCampaignRecord,
        window: &BackfillRefillWindow,
    ) -> Result<()> {
        // The engine identity defines which files still need OCR, so it is read
        // per pass rather than snapshotted at startup: an engine upgrade must
        // not be applied by a provider still selecting against the old version.
        let Some(engine) = strife_db::get_ocr_engine_state(&self.pool)
            .await
            .context("load ocr engine state")?
        else {
            // Without a verified engine every file would compare as a version
            // mismatch and the whole library would be enqueued. Refuse instead.
            warn!(
                campaign_id = %campaign.id,
                "no verified OCR engine; skipping refill"
            );
            return Ok(());
        };
        let (enqueued, exhausted) = strife_db::enqueue_ocr_backfill_batch(
            &self.pool,
            campaign,
            &self.supported_mimes,
            Some(&engine.engine_version),
            window.allowance,
        )
        .await
        .context("enqueue ocr backfill batch")?;
        debug!(
            campaign_id = %campaign.id,
            enqueued,
            exhausted,
            allowance = window.allowance,
            "ocr backfill refill"
        );
        if exhausted && enqueued == 0 {
            // No candidates remain. Draining lets leased work finish before an
            // operator completes the campaign; it never cancels running jobs.
            strife_db::transition_backfill_campaign(
                &self.pool,
                campaign.id,
                BackfillState::Draining,
                Some("candidate set exhausted"),
            )
            .await
            .context("drain exhausted ocr campaign")?;
        }
        Ok(())
    }
}

/// Historical email adapter for the shared coordinator.
///
/// Mirrors `OcrBackfillProvider`: the enqueue-and-cursor transaction lives in
/// the database layer, and this type only supplies the parser identity that
/// defines which files still need parsing.
pub struct EmailBackfillProvider {
    pool: PgPool,
    parser_version: String,
}

impl EmailBackfillProvider {
    #[must_use]
    pub fn new(pool: PgPool) -> Self {
        Self {
            pool,
            parser_version: strife_media::EMAIL_PARSER_VERSION.to_owned(),
        }
    }
}

#[async_trait]
impl BackfillCandidateProvider for EmailBackfillProvider {
    async fn refill(
        &self,
        campaign: &BackfillCampaignRecord,
        window: &BackfillRefillWindow,
    ) -> Result<()> {
        let (enqueued, exhausted) = strife_db::enqueue_email_backfill_batch(
            &self.pool,
            campaign,
            Some(&self.parser_version),
            window.allowance,
        )
        .await
        .context("enqueue email backfill batch")?;
        debug!(
            campaign_id = %campaign.id,
            enqueued,
            exhausted,
            allowance = window.allowance,
            "email backfill refill"
        );
        if exhausted && enqueued == 0 {
            strife_db::transition_backfill_campaign(
                &self.pool,
                campaign.id,
                BackfillState::Draining,
                Some("candidate set exhausted"),
            )
            .await
            .context("drain exhausted email campaign")?;
        }
        Ok(())
    }
}

pub(crate) async fn coordinator_loop(
    pool: PgPool,
    interval: Duration,
    mut shutdown: watch::Receiver<bool>,
) -> Result<()> {
    let coordinator = BackfillCoordinator::new()
        .with_provider(
            BackfillKind::Ocr,
            Arc::new(OcrBackfillProvider::new(pool.clone())),
        )
        .with_provider(
            BackfillKind::Email,
            Arc::new(EmailBackfillProvider::new(pool.clone())),
        );
    loop {
        if *shutdown.borrow() {
            return Ok(());
        }
        if let Err(error) = coordinator.run_once(&pool).await {
            error!(%error, "backfill coordinator pass failed");
        }
        tokio::select! {
            () = tokio::time::sleep(interval) => {}
            changed = shutdown.changed() => {
                if changed.is_err() || *shutdown.borrow() {
                    return Ok(());
                }
            }
        }
    }
}
