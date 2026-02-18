id: '56'
title: 'Add validation to create --claim: require --acceptance or --verify'
slug: add-validation-to-create-claim-require-acceptance
status: closed
priority: 2
created_at: 2026-02-18T07:59:26.740545Z
updated_at: 2026-02-18T08:01:06.095146Z
acceptance: When bn create --claim is used (without --parent), it must require --acceptance or --verify, same as bn quick. Parent/goal beans (no --claim) remain exempt. Add a test confirming the error message.
closed_at: 2026-02-18T08:01:06.095146Z
verify: cd /Users/asher/beans && cargo test --test cli_tests -- create 2>&1 | grep -q "PASS\|ok"
claimed_at: 2026-02-18T07:59:26.778633Z
is_archived: true
tokens: 65
tokens_updated: 2026-02-18T07:59:26.744733Z
