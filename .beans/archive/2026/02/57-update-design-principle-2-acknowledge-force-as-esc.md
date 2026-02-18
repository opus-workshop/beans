id: '57'
title: 'Update Design Principle #2: acknowledge --force as escape hatch'
slug: update-design-principle-2-acknowledge-force-as-esc
status: closed
priority: 2
created_at: 2026-02-18T08:01:24.394431Z
updated_at: 2026-02-18T08:01:39.255419Z
acceptance: 'Design Principle #2 should still emphasize verify gates are the default, but not claim ''No force-close'' since --force exists. Keep it brief — don''t over-explain --force, just don''t contradict reality.'
closed_at: 2026-02-18T08:01:39.255419Z
verify: cd /Users/asher/beans && grep -q "force" README.md && ! grep -q "No force-close" README.md
claimed_at: 2026-02-18T08:01:24.411863Z
is_archived: true
tokens: 66
tokens_updated: 2026-02-18T08:01:24.395780Z
