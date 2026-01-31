use std::path::Path;

use anyhow::Result;

use crate::index::Index;

/// Force rebuild index unconditionally from YAML files
pub fn cmd_sync(beans_dir: &Path) -> Result<()> {
    let index = Index::build(beans_dir)?;
    let count = index.beans.len();
    index.save(beans_dir)?;

    println!("Index rebuilt: {} beans indexed.", count);

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bean::Bean;
    use crate::util::title_to_slug;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn sync_rebuilds_index() {
        let dir = TempDir::new().unwrap();
        let beans_dir = dir.path().join(".beans");
        fs::create_dir(&beans_dir).unwrap();

        let bean1 = Bean::new("1", "Task one");
        let bean2 = Bean::new("2", "Task two");

        let slug1 = title_to_slug(&bean1.title);
        let slug2 = title_to_slug(&bean2.title);

        bean1.to_file(beans_dir.join(format!("1-{}.md", slug1))).unwrap();
        bean2.to_file(beans_dir.join(format!("2-{}.md", slug2))).unwrap();

        // Sync should create index with 2 beans
        let result = cmd_sync(&beans_dir);
        assert!(result.is_ok());

        // Verify index was created
        assert!(beans_dir.join("index.yaml").exists());

        // Verify index contains both beans
        let index = Index::load(&beans_dir).unwrap();
        assert_eq!(index.beans.len(), 2);
    }

    #[test]
    fn sync_counts_beans() {
        let dir = TempDir::new().unwrap();
        let beans_dir = dir.path().join(".beans");
        fs::create_dir(&beans_dir).unwrap();

        // Create 5 beans
        for i in 1..=5 {
            let bean = Bean::new(i.to_string(), format!("Task {}", i));
            let slug = title_to_slug(&bean.title);
            bean.to_file(beans_dir.join(format!("{}-{}.md", i, slug)))
                .unwrap();
        }

        let result = cmd_sync(&beans_dir);
        assert!(result.is_ok());

        let index = Index::load(&beans_dir).unwrap();
        assert_eq!(index.beans.len(), 5);
    }

    #[test]
    fn sync_empty_project() {
        let dir = TempDir::new().unwrap();
        let beans_dir = dir.path().join(".beans");
        fs::create_dir(&beans_dir).unwrap();

        let result = cmd_sync(&beans_dir);
        assert!(result.is_ok());

        let index = Index::load(&beans_dir).unwrap();
        assert_eq!(index.beans.len(), 0);
    }
}
