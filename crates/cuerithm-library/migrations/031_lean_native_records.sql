-- Lean library: the library keeps tags, file locations and artwork, not copies of a DJ
-- program's database.
--
-- A rekordbox record keeps its identity and hash (that is all a later observation compares)
-- and keeps its content only in the tables native playlist sync reads back. A DJ-software
-- track that is not linked to a library track keeps the hash of its observation, not the
-- observation. The rekordbox database is read again on every sync.
UPDATE native_records SET json='' WHERE json!='' AND table_name NOT IN ('djmdPlaylist','djmdSongPlaylist','djmdMyTag','djmdSongMyTag');
UPDATE native_track_links SET observation_json=NULL WHERE local_track_id IS NULL AND observation_json IS NOT NULL AND observation_hash IS NOT NULL;
PRAGMA user_version=31;
