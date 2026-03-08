CREATE TABLE click_events (
    id SERIAL PRIMARY KEY,
    link_id INTEGER NOT NULL REFERENCES links(id) ON DELETE CASCADE,
    clicked_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    country TEXT,
    browser TEXT,
    os TEXT
);

CREATE INDEX click_events_link_id_idx ON click_events(link_id);
CREATE INDEX click_events_clicked_at_idx ON click_events(clicked_at);
