-- Baseline tag vocabulary. Users can add their own; these exist so a fresh install has
-- useful filters on day one. Ids are deterministic so re-running migrations on a copied
-- database cannot create duplicates under a different id.
INSERT INTO tags (id, name, color, created_at) VALUES
    ('tag-aws',         'AWS',                 '#ff9900', '2026-01-01T00:00:00Z'),
    ('tag-azure',       'Azure',               '#0078d4', '2026-01-01T00:00:00Z'),
    ('tag-gcp',         'GCP',                 '#34a853', '2026-01-01T00:00:00Z'),
    ('tag-linux',       'Linux',               '#f2c94c', '2026-01-01T00:00:00Z'),
    ('tag-windows',     'Windows',             '#2f80ed', '2026-01-01T00:00:00Z'),
    ('tag-network',     'Network',             '#56ccf2', '2026-01-01T00:00:00Z'),
    ('tag-web',         'Web',                 '#9b51e0', '2026-01-01T00:00:00Z'),
    ('tag-cloud',       'Cloud',               '#00b8d9', '2026-01-01T00:00:00Z'),
    ('tag-incident',    'Incident',            '#eb5757', '2026-01-01T00:00:00Z'),
    ('tag-vuln',        'Vulnerability',       '#f2994a', '2026-01-01T00:00:00Z'),
    ('tag-compliance',  'Compliance',          '#219653', '2026-01-01T00:00:00Z'),
    ('tag-pentest',     'Penetration Testing', '#bb6bd9', '2026-01-01T00:00:00Z'),
    ('tag-postmortem',  'Postmortem',          '#828282', '2026-01-01T00:00:00Z');
