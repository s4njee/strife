use std::{collections::HashMap, sync::Arc, time::Duration};

use anyhow::{Context, Result};
use async_trait::async_trait;
use sqlx::PgPool;
use strife_db::{BackfillCampaignRecord, BackfillKind, BackfillRefillWindow, BackfillState};
use tokio::sync::watch;
use tracing::{debug, error};

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
            if window.allowance > 0 {
                provider.refill(&campaign, &window).await?;
            }
        }
        Ok(())
    }
}

pub(crate) async fn coordinator_loop(
    pool: PgPool,
    interval: Duration,
    mut shutdown: watch::Receiver<bool>,
) -> Result<()> {
    // Phase 2 intentionally registers no feature adapter. OCR plugs in during
    // Story 16.6; until then even BACKFILL_ENABLED cannot enumerate files.
    let coordinator = BackfillCoordinator::new();
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
