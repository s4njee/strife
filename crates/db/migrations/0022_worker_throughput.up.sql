INSERT INTO worker_resource_leases (resource_class, slot_number)
VALUES ('extractor', 2)
ON CONFLICT DO NOTHING;
