-- The pack a LIBRARY story was last written as on a device ("Envoyer vers la
-- Lunii"): the durable story ↔ pack-UUID link in the library → device
-- direction, the mirror of `story_imports` (device → library). Lets the
-- device inventory recognize a pack sent from this library (its local story,
-- its title) and lets the library stamp a story as already on the connected
-- device — whatever the pack's origin (a synthesized pack is named by the
-- story id, an archive pack by its own entry uuid). One row per story, the
-- LAST send wins; cascades with the story.
CREATE TABLE story_device_packs (
  story_id  TEXT PRIMARY KEY,
  pack_uuid TEXT NOT NULL,
  sent_at   TEXT NOT NULL,
  FOREIGN KEY (story_id) REFERENCES stories(id) ON DELETE CASCADE,
  CHECK (length(pack_uuid) = 36)
);
CREATE INDEX story_device_packs_pack_uuid ON story_device_packs(pack_uuid);
