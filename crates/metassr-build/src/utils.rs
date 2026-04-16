use anyhow::{anyhow, Result};
use std::{
    collections::HashMap,
    ffi::OsStr,
    path::{Path, PathBuf},
};

pub fn setup_page_path(page: &str, ext: &str) -> PathBuf {
    match Path::new(page) {
        path if path.file_stem() != Some(OsStr::new("index")) => path
            .to_path_buf()
            .with_extension("")
            .join(format!("index.{ext}")),

        path => path.to_path_buf().with_extension(ext),
    }
}

pub fn filter_target_pages(
    target_pages: &Option<Vec<String>>,
    all_pages: HashMap<String, PathBuf>,
) -> Result<HashMap<String, PathBuf>> {
    match target_pages {
        Some(targets) => {
            let mut filtered = HashMap::new();
            for target in targets {
                match all_pages.get(target) {
                    Some(path) => {
                        filtered.insert(target.clone(), path.clone());
                    }
                    None => {
                        return Err(anyhow!(
                            "Target page '{}' not found in source directory",
                            target
                        ))
                    }
                }
            }
            Ok(filtered)
        }
        None => Ok(all_pages),
    }
}
