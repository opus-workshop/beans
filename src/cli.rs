use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(
    name = "bn",
    about = "A hierarchical task engine where every task is a YAML file",
    version
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Subcommand)]
pub enum Command {
    /// Initialize .beans/ in the current directory
    Init {
        /// Project name (auto-detected from directory if omitted)
        name: Option<String>,
    },

    /// Create a new bean
    #[command(visible_alias = "new")]
    Create {
        /// Bean title
        title: Option<String>,

        /// Bean title (alternative to positional arg)
        #[arg(long)]
        #[arg(conflicts_with = "title")]
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

        /// Comma-separated dependency IDs
        #[arg(long)]
        deps: Option<String>,
    },

    /// Display full bean details
    #[command(visible_alias = "view")]
    Show {
        /// Bean ID
        id: String,

        /// Output as JSON
        #[arg(long)]
        json: bool,

        /// One-line summary
        #[arg(long)]
        short: bool,
    },

    /// List beans with filtering
    #[command(visible_alias = "ls")]
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
    },

    /// Update bean fields
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

        /// New/appended notes
        #[arg(long)]
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
    Close {
        /// Bean IDs
        #[arg(required = true)]
        ids: Vec<String>,

        /// Close reason
        #[arg(long)]
        reason: Option<String>,
    },

    /// Run a bean's verify command without closing
    Verify {
        /// Bean ID
        id: String,
    },

    /// Reopen a closed bean
    Reopen {
        /// Bean ID
        id: String,
    },

    /// Delete a bean and clean up references
    Delete {
        /// Bean ID
        id: String,
    },

    /// Manage dependencies between beans
    Dep {
        #[command(subcommand)]
        command: DepCommand,
    },

    /// Show beans ready to work on (no blocking dependencies)
    Ready,

    /// Show beans blocked by unresolved dependencies
    Blocked,

    /// Show hierarchical tree of beans
    Tree {
        /// Root bean ID (shows full tree if omitted)
        id: Option<String>,
    },

    /// Display dependency graph
    Graph {
        /// Output format: ascii (default), mermaid, dot
        #[arg(long, default_value = "ascii")]
        format: String,
    },

    /// Force rebuild index from YAML files
    Sync,

    /// Project statistics
    Stats,

    /// Claim a bean for work (sets status to in_progress)
    Claim {
        /// Bean ID
        id: String,

        /// Release the claim instead of acquiring it
        #[arg(long)]
        release: bool,

        /// Who is claiming (agent name or user)
        #[arg(long)]
        by: Option<String>,
    },

    /// Health check -- orphans, cycles, index freshness
    Doctor,

    /// Unarchive a bean (move from archive back to main beans directory)
    Unarchive {
        /// Bean ID to unarchive
        id: String,
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
