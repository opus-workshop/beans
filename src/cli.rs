use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(
    name = "bn",
    about = "Task tracker for coding agents",
    version,
    help_template = "\
{about-with-newline}
Usage: {usage}

Commands:
  TASKS
    init         Initialize .beans/ in the current directory
    create       Create a new bean [aliases: new]
    quick        Quick-create: create a bean and immediately claim it [aliases: q]
    show         Display full bean details [aliases: view]
    list         List beans with filtering [aliases: ls]
    edit         Edit bean in $EDITOR
    update       Update bean fields
    claim        Claim a bean for work (sets status to in_progress)
    close        Close one or more beans (runs verify gate first)
    verify       Run a bean's verify command without closing
    reopen       Reopen a closed bean
    delete       Delete a bean and clean up references

  QUERY
    status       Show project status: claimed, ready, and blocked beans
    ready        Show beans ready to work on (no blocking dependencies)
    blocked      Show beans blocked by unresolved dependencies
    tree         Show hierarchical tree of beans
    graph        Display dependency graph
    context      Output context for a bean, or memory context (no args)
    trace        Walk bean lineage and dependency chain

  MEMORY
    fact         Create a verified fact (requires --verify)
    recall       Search beans by keyword
    verify-facts Re-verify all facts, detect staleness

  AGENTS
    run          Dispatch ready beans to agents
    plan         Interactively plan a large bean into children
    review       Adversarial post-close review of an implementation
    agents       Show running and recently completed agents
    logs         View agent output from log files

  MCP
    mcp          MCP server for IDE integration (Cursor, Windsurf, Claude Desktop, Cline)

  DEPENDENCIES
    dep          Manage dependencies between beans
    adopt        Adopt existing beans as children of a parent

  MAINTENANCE
    tidy         Archive closed beans, release stale in-progress beans
    sync         Force rebuild index from YAML files
    doctor       Health check -- orphans, cycles, index freshness
    stats        Project statistics
    config       Manage project configuration
    trust        Manage hook trust (enable/disable hook execution)
    unarchive    Unarchive a bean (move from archive back to main beans directory)
    locks        View and manage file locks for concurrent agents

  SHELL
    completions  Generate shell completions (bash, zsh, fish, powershell)
  help         Print this message or the help of the given subcommand(s)

{options}
Getting started:
  bn init                                         Initialize .beans/ in this directory
  bn create \"fix bug\" --verify \"cargo test auth\"  Create a task with a verify gate
  bn run                                          Dispatch ready beans to agents
  bn status                                       See what's in flight

See 'bn <command> --help' for details and examples."
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Subcommand)]
pub enum Command {
    // -- TASKS --
    /// Initialize .beans/ in the current directory
    ///
    /// Creates .beans/ with config.yaml and sets up agent command templates.
    /// Agent presets (pi, claude, aider) auto-configure the run/plan commands.
    /// Use --setup on an existing project to reconfigure the agent.
    #[command(
        display_order = 1,
        after_help = "\
Examples:
  bn init                     Interactive setup
  bn init --agent pi          Use pi agent preset
  bn init --agent claude      Use Claude Code preset
  bn init myproject           Name the project explicitly
  bn init --setup             Reconfigure agent on existing project"
    )]
    Init {
        /// Project name (auto-detected from directory if omitted)
        name: Option<String>,

        /// Use a known agent preset (pi, claude, aider)
        #[arg(long)]
        agent: Option<String>,

        /// Custom run command template (use {id} for bean ID)
        #[arg(long)]
        run: Option<String>,

        /// Custom plan command template (use {id} for bean ID)
        #[arg(long)]
        plan: Option<String>,

        /// Reconfigure agent on existing project
        #[arg(long)]
        setup: bool,

        /// Skip agent setup
        #[arg(long)]
        no_agent: bool,
    },

    /// Create a new bean
    ///
    /// Every bean needs a verify gate (--verify) — a shell command that must exit 0
    /// to close the bean. The --description is the agent's prompt when dispatched via
    /// `bn run`: include concrete steps, file paths, embedded types/signatures, and
    /// what NOT to do.
    ///
    /// Use -p (--pass-ok) when verify already passes (refactors, docs, type changes).
    /// Use --parent to create child beans under a larger parent task.
    /// Use --produces/--requires to set up artifact-based dependency ordering.
    #[command(
        visible_alias = "new",
        display_order = 2,
        args_conflicts_with_subcommands = true,
        after_help = "\
Examples:
  bn create \"fix login bug\" --verify \"cargo test auth::login\"
  bn create \"add tests\" --verify \"pytest tests/auth.py\" -p
  bn create \"refactor API\" --verify \"cargo build\" --description \"## Task\\n...\"
  bn create \"add endpoint\" --parent 5 --verify \"cargo test\" --produces \"UserAPI\"
  bn create next \"step 2\" --verify \"cargo test\"   (auto-depends on last bean)

Verify patterns:
  Rust     cargo test module::test_name
  JS/TS    npx vitest run path/to/test
  Python   pytest tests/file.py -k test_name
  Go       go test ./pkg -run TestName
  Check    grep -q 'expected' file.txt
  Remove   ! grep -rq 'old_pattern' src/
  Multi    cmd1 && cmd2 && cmd3"
    )]
    Create {
        #[command(flatten)]
        args: Box<CreateOpts>,
    },

    /// Display full bean details
    ///
    /// Shows all fields: title, description, verify command, status, dependencies,
    /// history, and notes. Use --short for a one-line summary.
    #[command(visible_alias = "view", display_order = 4)]
    Show {
        /// Bean ID
        id: String,

        /// Output as JSON
        #[arg(long)]
        json: bool,

        /// One-line summary
        #[arg(long)]
        short: bool,

        /// Show all history entries (default: last 10)
        #[arg(long)]
        history: bool,
    },

    /// List beans with filtering
    ///
    /// By default shows open and in-progress beans. Use --all to include closed.
    /// Combine filters to narrow results. Use --ids for piping to other commands.
    #[command(
        visible_alias = "ls",
        display_order = 5,
        after_help = "\
Examples:
  bn ls                              All open/in-progress beans
  bn ls --all                        Include closed beans
  bn ls --status in_progress         Only claimed beans
  bn ls --label bug --priority 0     High-priority bugs
  bn ls --parent 5                   Children of bean 5
  bn ls --ids | xargs -I{} bn show {}   Pipe to other commands
  bn ls --format '{id}\\t{title}'     Custom output format"
    )]
    List {
        /// Filter by status (open, in_progress, closed)
        #[arg(long)]
        status: Option<String>,

        /// Filter by priority
        #[arg(long)]
        priority: Option<u8>,

        /// Show children of a parent
        #[arg(long)]
        parent: Option<String>,

        /// Filter by label
        #[arg(long)]
        label: Option<String>,

        /// Filter by assignee
        #[arg(long)]
        assignee: Option<String>,

        /// Include closed beans
        #[arg(long)]
        all: bool,

        /// JSON output
        #[arg(long)]
        json: bool,

        /// Output only bean IDs (one per line, for piping)
        #[arg(long, conflicts_with = "json")]
        ids: bool,

        /// Custom output format (e.g. '{id}\t{title}\t{status}')
        #[arg(long, conflicts_with_all = ["json", "ids"])]
        format: Option<String>,
    },

    /// Edit bean in $EDITOR
    #[command(display_order = 6)]
    Edit {
        /// Bean ID
        id: String,
    },

    /// Update bean fields
    ///
    /// Use --note to log progress during work. Notes are timestamped and appended —
    /// they survive retries, so the next agent reads what was tried and what failed.
    /// Essential for debugging repeated failures.
    #[command(
        display_order = 7,
        after_help = "\
Examples:
  bn update 5 --note \"Completed auth module, starting tests\"
  bn update 5 --note \"Failed: JWT lib incompatible. Avoid: jsonwebtoken 8.x\"
  bn update 5 --priority 0
  bn update 5 --title \"Revised scope\" --add-label bug"
    )]
    Update {
        /// Bean ID
        id: String,

        /// New title
        #[arg(long)]
        title: Option<String>,

        /// New description
        #[arg(long)]
        description: Option<String>,

        /// New acceptance criteria
        #[arg(long)]
        acceptance: Option<String>,

        /// Append a note (with timestamp separator)
        #[arg(long, visible_alias = "note")]
        notes: Option<String>,

        /// New design notes
        #[arg(long)]
        design: Option<String>,

        /// New status (open, in_progress, closed)
        #[arg(long)]
        status: Option<String>,

        /// New priority
        #[arg(long)]
        priority: Option<u8>,

        /// New assignee
        #[arg(long)]
        assignee: Option<String>,

        /// Add a label
        #[arg(long)]
        add_label: Option<String>,

        /// Remove a label
        #[arg(long)]
        remove_label: Option<String>,
    },

    /// Close one or more beans (runs verify gate first)
    ///
    /// Runs the bean's verify command first — if it exits 0, the bean is closed.
    /// If verify fails, the close is rejected unless --force is used.
    /// Multiple IDs can be passed to batch-close.
    ///
    /// Use --failed to mark an attempt as explicitly failed (agent giving up).
    /// The bean stays open and the claim is released for another agent to retry.
    #[command(
        display_order = 9,
        after_help = "\
Examples:
  bn close 5                              Close after verify passes
  bn close 5 6 7                          Batch close
  bn close --force 5                      Skip verify (force close)
  bn close --failed 5 --reason \"blocked\"  Mark attempt as failed
  bn ls --ids | bn close --stdin          Close all listed beans"
    )]
    Close {
        /// Bean IDs (or use --stdin to read from pipe)
        #[arg(required_unless_present = "stdin")]
        ids: Vec<String>,

        /// Close reason
        #[arg(long)]
        reason: Option<String>,

        /// Skip verify command (force close)
        #[arg(long, conflicts_with = "failed")]
        force: bool,

        /// Mark attempt as failed (release claim, bean stays open)
        #[arg(long)]
        failed: bool,

        /// Read bean IDs from stdin (one per line)
        #[arg(long)]
        stdin: bool,
    },

    /// Run a bean's verify command without closing
    #[command(display_order = 10)]
    Verify {
        /// Bean ID
        id: String,

        /// Output result as JSON
        #[arg(long)]
        json: bool,
    },

    /// Reopen a closed bean
    #[command(display_order = 11)]
    Reopen {
        /// Bean ID
        id: String,
    },

    /// Delete a bean and clean up references
    #[command(display_order = 12)]
    Delete {
        /// Bean ID
        id: String,
    },

    // -- DEPENDENCIES --
    /// Manage dependencies between beans
    #[command(display_order = 30)]
    Dep {
        #[command(subcommand)]
        command: DepCommand,
    },
    // -- QUERY --
    /// Show beans ready to work on (no blocking dependencies)
    #[command(display_order = 21)]
    Ready {
        /// JSON output
        #[arg(long)]
        json: bool,
    },

    /// Show beans blocked by unresolved dependencies
    #[command(display_order = 22)]
    Blocked {
        /// JSON output
        #[arg(long)]
        json: bool,
    },

    /// Show project status: claimed, ready, and blocked beans
    ///
    /// Quick overview of what's in flight, what's ready for dispatch, and what's
    /// waiting on dependencies. Start here to understand project state.
    #[command(display_order = 20)]
    Status {
        /// JSON output
        #[arg(long)]
        json: bool,
    },

    /// Output context for a bean, or memory context (no args)
    ///
    /// With a bean ID: extracts and displays file contents referenced in the bean's
    /// description. Without an ID: outputs memory context — stale facts, currently
    /// claimed beans, and recent completions. Useful for agents to understand current
    /// project state before starting work.
    #[command(
        display_order = 25,
        after_help = "\
Examples:
  bn context         Memory context (stale facts, in-progress, recent work)
  bn context 5       File context for bean 5 (reads referenced files)
  bn context --json  Machine-readable memory context"
    )]
    Context {
        /// Bean ID (omit for memory context)
        id: Option<String>,

        /// Output as JSON (file paths and contents)
        #[arg(long)]
        json: bool,

        /// Output only the structural summary (signatures, imports) — skip full file contents
        #[arg(long)]
        structure_only: bool,
    },

    /// Show hierarchical tree of beans
    #[command(display_order = 23)]
    Tree {
        /// Root bean ID (shows full tree if omitted)
        id: Option<String>,
    },

    /// Display dependency graph
    #[command(display_order = 24)]
    Graph {
        /// Output format: ascii (default), mermaid, dot
        #[arg(long, default_value = "ascii")]
        format: String,
    },

    // -- MAINTENANCE --
    /// Force rebuild index from YAML files
    #[command(display_order = 41)]
    Sync,

    /// Archive closed beans, release stale in-progress beans, and rebuild the index
    #[command(display_order = 40)]
    Tidy {
        /// Show what would happen without changing any files
        #[arg(long)]
        dry_run: bool,
    },

    /// Project statistics
    #[command(display_order = 43)]
    Stats {
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },

    /// Claim a bean for work (sets status to in_progress)
    #[command(display_order = 8)]
    Claim {
        /// Bean ID
        id: String,

        /// Release the claim instead of acquiring it
        #[arg(long)]
        release: bool,

        /// Who is claiming (agent name or user)
        #[arg(long)]
        by: Option<String>,

        /// Force claim even if verify already passes
        #[arg(long)]
        force: bool,
    },

    /// Health check -- orphans, cycles, index freshness
    #[command(display_order = 42)]
    Doctor {
        /// Automatically fix detected issues
        #[arg(long)]
        fix: bool,
    },

    /// Manage hook trust (enable/disable hook execution)
    #[command(display_order = 44)]
    Trust {
        /// Revoke trust (disable hooks)
        #[arg(long)]
        revoke: bool,

        /// Check current trust status
        #[arg(long)]
        check: bool,
    },

    /// Unarchive a bean (move from archive back to main beans directory)
    #[command(display_order = 45)]
    Unarchive {
        /// Bean ID to unarchive
        id: String,
    },

    /// View and manage file locks for concurrent agents
    #[command(display_order = 46)]
    Locks {
        /// Force-clear all locks
        #[arg(long)]
        clear: bool,
    },

    /// Quick-create: create a bean and immediately claim it
    ///
    /// Use when you'll work on the bean yourself rather than dispatching to an agent.
    /// Equivalent to `bn create "..." --claim`. For agent dispatch, use `bn create` instead.
    #[command(
        visible_alias = "q",
        display_order = 3,
        after_help = "\
Examples:
  bn quick \"fix typo in README\" --verify \"grep -q 'correct text' README.md\"
  bn quick \"add logging\" -p   (verify already passes — refactor)"
    )]
    Quick {
        /// Bean title
        title: String,

        /// Full description / agent context
        #[arg(long)]
        description: Option<String>,

        /// Acceptance criteria
        #[arg(long)]
        acceptance: Option<String>,

        /// Additional notes
        #[arg(long)]
        notes: Option<String>,

        /// Shell command that must exit 0 to close
        #[arg(long)]
        verify: Option<String>,

        /// Priority P0-P4 (default: P2)
        #[arg(long)]
        priority: Option<u8>,

        /// Who is claiming (agent name or user)
        #[arg(long)]
        by: Option<String>,

        /// Comma-separated artifacts this bean produces
        #[arg(long)]
        produces: Option<String>,

        /// Comma-separated artifacts this bean requires
        #[arg(long)]
        requires: Option<String>,

        /// Parent bean ID (creates child bean under parent)
        #[arg(long)]
        parent: Option<String>,

        /// Action on verify failure: retry, retry:N, escalate, escalate:P0
        #[arg(long)]
        on_fail: Option<String>,

        /// Skip fail-first check (allow verify to already pass)
        #[arg(long, short = 'p')]
        pass_ok: bool,

        /// Timeout in seconds for the verify command (kills process on expiry)
        #[arg(long)]
        verify_timeout: Option<u64>,
    },

    /// Adopt existing beans as children of a parent
    #[command(display_order = 31)]
    Adopt {
        /// Parent bean ID
        parent: String,

        /// Bean IDs to adopt as children
        #[arg(required = true)]
        children: Vec<String>,
    },

    /// Dispatch ready beans to agents
    ///
    /// Without an ID, finds all ready beans (open, no unresolved deps) and spawns
    /// agents in parallel up to -j limit. With an ID, dispatches that specific bean.
    /// Agents run the command template from .beans/config.yaml (set via `bn init`).
    ///
    /// Use --loop-mode for continuous dispatch until all work is done — it re-scans
    /// for newly-ready beans after each wave completes. Use --auto-plan to automatically
    /// break down large beans before dispatching.
    #[command(after_help = "\
Examples:
  bn run              Dispatch all ready beans (up to -j 4 parallel)
  bn run 5            Dispatch a specific bean
  bn run --loop-mode  Keep going until no ready beans remain
  bn run --dry-run    Preview what would be dispatched
  bn run -j 8 --keep-going --timeout 60   High-throughput mode")]
    Run {
        /// Bean ID. Without ID, processes all ready beans.
        id: Option<String>,

        /// Max parallel agents
        #[arg(short = 'j', long, default_value = "4")]
        jobs: u32,

        /// Show plan without spawning
        #[arg(long)]
        dry_run: bool,

        /// Keep running until no ready beans remain
        #[arg(long, name = "loop")]
        loop_mode: bool,

        /// Also plan large beans autonomously
        #[arg(long)]
        auto_plan: bool,

        /// Continue past failures
        #[arg(long)]
        keep_going: bool,

        /// Max time per agent in minutes
        #[arg(long, default_value = "30")]
        timeout: u32,

        /// Kill agent if no output for N minutes
        #[arg(long, default_value = "5")]
        idle_timeout: u32,

        /// Emit JSON stream events to stdout (for programmatic consumers)
        #[arg(long)]
        json_stream: bool,

        /// Run adversarial review after each successful close
        #[arg(long)]
        review: bool,
    },

    /// Interactively plan a large bean into children
    ///
    /// Breaks a large bean into smaller child beans with proper dependencies.
    /// Use when a bean touches too many files or would take a single agent too long.
    /// Strategies: feature (by capability), layer (by architecture layer),
    /// phase (sequential steps), file (one bean per file to change).
    #[command(after_help = "\
Examples:
  bn plan 5                    Interactive breakdown of bean 5
  bn plan --auto               Autonomous planning (no prompts)
  bn plan 5 --strategy layer   Suggest layer-based split
  bn plan 5 --dry-run          Preview without creating children")]
    Plan {
        /// Bean ID to plan (omit to pick automatically)
        id: Option<String>,

        /// Suggest a split strategy (feature, layer, phase, file)
        #[arg(long)]
        strategy: Option<String>,

        /// Non-interactive autonomous planning
        #[arg(long)]
        auto: bool,

        /// Plan even if bean is below max_tokens
        #[arg(long)]
        force: bool,

        /// Show proposed split without creating
        #[arg(long)]
        dry_run: bool,
    },

    /// Show running and recently completed agents
    #[command(display_order = 37)]
    Agents {
        /// JSON output
        #[arg(long)]
        json: bool,
    },

    /// View agent output from log files
    ///
    /// Shows the agent's stdout/stderr from its most recent run. Use --all to see
    /// output from all runs (helpful when debugging repeated failures). Use -f to
    /// follow live output while an agent is running.
    #[command(
        display_order = 38,
        after_help = "\
Examples:
  bn logs 5          Latest run output
  bn logs 5 --all    All runs (for debugging retries)
  bn logs 5 -f       Follow live output"
    )]
    Logs {
        /// Bean ID
        id: String,

        /// Follow output (tail -f)
        #[arg(short, long)]
        follow: bool,

        /// Show all runs, not just latest
        #[arg(long)]
        all: bool,
    },

    // -- AGENTS --
    /// Manage project configuration
    #[command(display_order = 35)]
    Config {
        #[command(subcommand)]
        command: ConfigCommand,
    },

    // -- MCP --
    /// MCP server for IDE integration (Cursor, Windsurf, Claude Desktop, Cline, etc.)
    #[command(display_order = 60)]
    Mcp {
        #[command(subcommand)]
        command: McpCommand,
    },

    // -- MEMORY --
    /// Create a verified fact (requires --verify)
    ///
    /// Facts are verified truths about the project that persist across agent sessions.
    /// Each fact has a verify command that proves it's still true, and a TTL (default
    /// 30 days). Stale facts appear in `bn context` output. Re-check all facts with
    /// `bn verify-facts`. Good facts capture things agents need but can't infer from code.
    #[command(
        display_order = 50,
        after_help = "\
Examples:
  bn fact \"API uses Axum 0.8\" --verify \"grep -q 'axum = \\\"0.8' Cargo.toml\"
  bn fact \"Auth tokens expire after 24h\" --verify \"grep -q '24 * 60' src/config.rs\"
  bn fact \"Tests require Docker\" --verify \"docker info >/dev/null 2>&1\" --ttl 90"
    )]
    Fact {
        /// Fact title (what is true)
        title: String,

        /// Shell command that verifies this fact (required)
        #[arg(long)]
        verify: String,

        /// Description / additional context
        #[arg(long)]
        description: Option<String>,

        /// Comma-separated file paths this fact is relevant to
        #[arg(long)]
        paths: Option<String>,

        /// Time-to-live in days before fact becomes stale (default: 30)
        #[arg(long)]
        ttl: Option<i64>,

        /// Skip fail-first check
        #[arg(long, short = 'p')]
        pass_ok: bool,
    },

    /// Search beans by keyword
    ///
    /// Searches titles, descriptions, and notes. Use --all to include closed/archived beans.
    #[command(
        display_order = 51,
        after_help = "\
Examples:
  bn recall \"auth\"           Search open beans
  bn recall \"JWT\" --all      Include closed/archived
  bn recall \"login\" --json   Machine-readable results"
    )]
    Recall {
        /// Search query
        query: String,

        /// Include closed/archived beans
        #[arg(long)]
        all: bool,

        /// JSON output
        #[arg(long)]
        json: bool,
    },

    /// Re-verify all facts, detect staleness
    #[command(display_order = 52, name = "verify-facts")]
    VerifyFacts,

    // -- TRACE --
    /// Walk bean lineage and dependency chain
    ///
    /// Shows the full context for a bean: parent chain up to root, direct children,
    /// what it depends on, what depends on it, artifacts produced/required, and
    /// a summary of all agent attempts.
    #[command(
        display_order = 41,
        after_help = "\
Examples:
  bn trace 7.3              Show full trace for bean 7.3
  bn trace 7.3 --json       Machine-readable JSON output"
    )]
    Trace {
        /// Bean ID to trace
        id: String,

        /// Output as JSON
        #[arg(long)]
        json: bool,
    },

    /// Adversarial post-close review of a bean's implementation
    ///
    /// Spawns a review agent with the bean's spec + current git diff as context.
    /// The review agent outputs a verdict: approve, request-changes, or flag.
    ///
    ///   approve        — labels bean as `reviewed`
    ///   request-changes — reopens bean with review notes, labels `review-failed`
    ///   flag           — labels bean `needs-human-review`, stays closed
    ///
    /// Configure the review agent in .beans/config.yaml:
    ///   review:
    ///     run: "pi -p 'review bean {id}: ...'"
    ///     max_reopens: 2
    ///
    /// Falls back to the global `run` template if review.run is not set.
    /// Use `bn run --review` to auto-review after every close during a run.
    #[command(
        display_order = 39,
        after_help = "\
Examples:
  bn review 5                  Review bean 5's implementation
  bn review 5 --diff           Include only git diff (no spec)
  bn review 5 --model claude   Use a specific model
  bn run --review              Auto-review after each close"
    )]
    Review {
        /// Bean ID to review
        id: String,

        /// Include only the git diff, not the full bean description
        #[arg(long)]
        diff: bool,

        /// Override model for the review agent
        #[arg(long)]
        model: Option<String>,
    },

    // -- SHELL COMPLETIONS --
    /// Generate shell completions
    ///
    /// Prints a completion script to stdout. Add to your shell's rc file:
    ///   bash:  eval "$(bn completions bash)"
    ///   zsh:   eval "$(bn completions zsh)"
    ///   fish:  bn completions fish | source
    #[command(
        display_order = 70,
        after_help = "\
Examples:
  bn completions bash              Print bash completions
  bn completions zsh               Print zsh completions
  bn completions fish              Print fish completions
  bn completions powershell        Print PowerShell completions

Install permanently:
  bash:  echo 'eval \"$(bn completions bash)\"' >> ~/.bashrc
  zsh:   echo 'eval \"$(bn completions zsh)\"' >> ~/.zshrc
  fish:  bn completions fish > ~/.config/fish/completions/bn.fish"
    )]
    Completions {
        /// Shell to generate completions for
        #[arg(value_enum)]
        shell: clap_complete::Shell,
    },
}

#[derive(Subcommand)]
pub enum DepCommand {
    /// Add a dependency (id depends on depends-on-id)
    Add {
        /// Bean ID that will have the dependency
        id: String,

        /// Bean ID that must be completed first
        depends_on: String,
    },

    /// Remove a dependency
    Remove {
        /// Bean ID
        id: String,

        /// Dependency to remove
        depends_on: String,
    },

    /// Show dependencies and dependents of a bean
    List {
        /// Bean ID
        id: String,
    },

    /// Show full dependency tree
    Tree {
        /// Root bean ID (shows project-wide DAG if omitted)
        id: Option<String>,
    },

    /// Detect dependency cycles in the graph
    Cycles,
}

#[derive(Subcommand)]
pub enum ConfigCommand {
    /// Get a configuration value
    Get {
        /// Config key (run, plan, max_tokens, max_concurrent, poll_interval, auto_close_parent, max_loops, rules_file, file_locking, verify_timeout, extends, on_close, on_fail, post_plan, review.run, review.max_reopens)
        key: String,
    },

    /// Set a configuration value
    Set {
        /// Config key (run, plan, max_tokens, max_concurrent, poll_interval, auto_close_parent, max_loops, rules_file, file_locking, verify_timeout, extends, on_close, on_fail, post_plan, review.run, review.max_reopens)
        key: String,

        /// New value
        value: String,
    },
}

#[derive(Subcommand)]
pub enum McpCommand {
    /// Start MCP server on stdio (JSON-RPC 2.0)
    Serve,
}

#[derive(Subcommand)]
pub enum CreateSubcommand {
    /// Create a bean that depends on the most recently created bean (sequential chaining)
    ///
    /// Automatically adds a dependency on @latest, enabling easy sequential chains:
    ///   bn create "Step 1" -p
    ///   bn create next "Step 2" --verify "cargo test step2"
    ///   bn create next "Step 3" --verify "cargo test step3"
    Next {
        /// Bean title
        title: Option<String>,

        /// Bean title (alternative to positional arg)
        #[arg(long, conflicts_with = "title")]
        set_title: Option<String>,

        /// Full description / agent context
        #[arg(long)]
        description: Option<String>,

        /// Acceptance criteria
        #[arg(long)]
        acceptance: Option<String>,

        /// Additional notes
        #[arg(long)]
        notes: Option<String>,

        /// Design decisions
        #[arg(long)]
        design: Option<String>,

        /// Shell command that must exit 0 to close
        #[arg(long)]
        verify: Option<String>,

        /// Parent bean ID -- child gets next dot-number
        #[arg(long)]
        parent: Option<String>,

        /// Priority P0-P4 (default: P2)
        #[arg(long)]
        priority: Option<u8>,

        /// Comma-separated labels
        #[arg(long)]
        labels: Option<String>,

        /// Assignee name
        #[arg(long)]
        assignee: Option<String>,

        /// Additional comma-separated dependency IDs (merged with auto @latest dep)
        #[arg(long)]
        deps: Option<String>,

        /// Comma-separated artifacts this bean produces
        #[arg(long)]
        produces: Option<String>,

        /// Comma-separated artifacts this bean requires
        #[arg(long)]
        requires: Option<String>,

        /// Action on verify failure: retry, retry:N, escalate, escalate:P0
        #[arg(long)]
        on_fail: Option<String>,

        /// Skip fail-first check (allow verify to already pass)
        #[arg(long, short = 'p')]
        pass_ok: bool,

        /// Timeout in seconds for the verify command (kills process on expiry)
        #[arg(long)]
        verify_timeout: Option<u64>,

        /// Claim the bean immediately (sets status to in_progress)
        #[arg(long, conflicts_with = "run")]
        claim: bool,

        /// Who is claiming (requires --claim)
        #[arg(long, requires = "claim")]
        by: Option<String>,

        /// Spawn an agent to work on this bean (requires --verify)
        #[arg(long)]
        run: bool,

        /// Output created bean as JSON (for piping)
        #[arg(long)]
        json: bool,
    },
}

#[derive(clap::Args)]
pub struct CreateOpts {
    #[command(subcommand)]
    pub subcommand: Option<CreateSubcommand>,

    /// Bean title
    pub title: Option<String>,

    /// Bean title (alternative to positional arg)
    #[arg(long, conflicts_with = "title")]
    pub set_title: Option<String>,

    /// Full description / agent context
    #[arg(long)]
    pub description: Option<String>,

    /// Acceptance criteria
    #[arg(long)]
    pub acceptance: Option<String>,

    /// Additional notes
    #[arg(long)]
    pub notes: Option<String>,

    /// Design decisions
    #[arg(long)]
    pub design: Option<String>,

    /// Shell command that must exit 0 to close
    #[arg(long)]
    pub verify: Option<String>,

    /// Parent bean ID -- child gets next dot-number
    #[arg(long)]
    pub parent: Option<String>,

    /// Priority P0-P4 (default: P2)
    #[arg(long)]
    pub priority: Option<u8>,

    /// Comma-separated labels
    #[arg(long)]
    pub labels: Option<String>,

    /// Assignee name
    #[arg(long)]
    pub assignee: Option<String>,

    /// Comma-separated dependency IDs
    #[arg(long)]
    pub deps: Option<String>,

    /// Comma-separated artifacts this bean produces
    #[arg(long)]
    pub produces: Option<String>,

    /// Comma-separated artifacts this bean requires
    #[arg(long)]
    pub requires: Option<String>,

    /// Action on verify failure: retry, retry:N, escalate, escalate:P0
    #[arg(long)]
    pub on_fail: Option<String>,

    /// Skip fail-first check (allow verify to already pass)
    #[arg(long, short = 'p')]
    pub pass_ok: bool,

    /// Timeout in seconds for the verify command (kills process on expiry)
    #[arg(long)]
    pub verify_timeout: Option<u64>,

    /// Claim the bean immediately (sets status to in_progress)
    #[arg(long, conflicts_with = "run")]
    pub claim: bool,

    /// Who is claiming (requires --claim)
    #[arg(long, requires = "claim")]
    pub by: Option<String>,

    /// Spawn an agent to work on this bean (requires --verify)
    #[arg(long)]
    pub run: bool,

    /// Launch interactive wizard (prompts for all fields step-by-step)
    #[arg(long, short = 'i')]
    pub interactive: bool,

    /// Output created bean as JSON (for piping)
    #[arg(long)]
    pub json: bool,
}
