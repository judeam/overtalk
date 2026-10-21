-- Where a meeting's notice record comes from: this computer, or an .overtalk file
-- someone shared. A shared file's record is what its sender reported, not a notice
-- given here, and it is shown that way.
ALTER TABLE meeting_notices ADD COLUMN source TEXT NOT NULL DEFAULT 'this-computer'
 CHECK(source IN ('this-computer','shared-file'));
