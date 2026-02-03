id: '2'
title: 'BUG: Archived beans invisible to --status closed queries'
status: closed
priority: 2
created_at: 2026-02-02T10:00:00Z
updated_at: 2026-02-03T01:26:50.955724Z
description: "## Problem\nWhen beans are closed, they get archived to `.beans/archive/`. \nAfter archiving, `bn list --status closed` cannot find them.\n\nThis breaks parent bean verify commands like:\n```bash\nverify: test $(bn list --parent 211 --status closed | wc -l) -ge 8\n```\n\n## Reproduction\n```bash\nbn quick \"test\" --verify \"true\"\nbn close 1  # Archives to .beans/archive/\nbn list --status closed  # Empty or doesn't include archived bean\n```\n\n## Expected\nEither:\n1. `--status closed` should search archives too\n2. Or provide `--include-archived` flag\n3. Or don't archive on close (just mark status)\n\n## Impact\n- Parent beans can't verify all children closed\n- Queries for completed work fail\n"
closed_at: 2026-02-03T01:26:50.955724Z
close_reason: Added Index::collect_archived() to recursively walk .beans/archive/ and load archived beans. Modified cmd_list to include archived beans when --status closed or --all is used. Fixed filter logic to not exclude closed beans when explicitly filtering for them.
verify: 'cargo test archive_tests::'
claimed_at: 2026-02-03T01:21:53.494717Z
is_archived: true
