-- App database v2 (2.1): licensing country and unit on operator profiles; GPS source preferences.
ALTER TABLE profiles ADD COLUMN license_country TEXT NOT NULL DEFAULT '';
ALTER TABLE profiles ADD COLUMN unit TEXT NOT NULL DEFAULT '';
