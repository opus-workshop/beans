id: '58'
title: Mark verify-on-claim as planned in README
slug: mark-verify-on-claim-as-planned-in-readme
status: closed
priority: 2
created_at: 2026-02-18T08:01:42.830654Z
updated_at: 2026-02-18T08:02:00.157429Z
acceptance: Sections documenting verify-on-claim as working should be marked as planned/upcoming. Don't remove them — just make it clear this is not yet implemented. Bean 11 tracks the actual implementation.
closed_at: 2026-02-18T08:02:00.157429Z
verify: 'cd /Users/asher/beans && ! grep -q "Option 2: Automatic on claim" README.md || grep -A2 "Option 2" README.md | grep -qi "planned\|coming\|not yet"'
claimed_at: 2026-02-18T08:01:42.847662Z
is_archived: true
tokens: 59
tokens_updated: 2026-02-18T08:01:42.831942Z
