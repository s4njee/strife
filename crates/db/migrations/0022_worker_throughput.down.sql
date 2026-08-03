DELETE FROM worker_resource_leases
WHERE resource_class = 'extractor'
  AND slot_number = 2
  AND lease_owner IS NULL;
