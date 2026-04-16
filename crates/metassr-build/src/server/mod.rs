pub mod renderer;

pub mod manifest;
mod pages_generator;
mod render;
mod render_exec;
mod targets;

use crate::{traits::Build, utils::filter_target_pages};
use manifest::ManifestGenerator;

use metassr_bundler::WebBundler;
use metassr_fs_analyzer::{
    dist_dir::DistDir,
    src_dir::{special_entries, SourceDir},
    DirectoryAnalyzer,
};
use metassr_utils::cache_dir::CacheDir;

use pages_generator::PagesGenerator;
use renderer::head::HeadRenderer;

use std::{
    ffi::OsStr,
    fs,
    path::{Path, PathBuf},
};
use targets::TargetsGenerator;

use anyhow::{anyhow, Result};

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum BuildingType {
    ServerSideRendering,
    StaticSiteGeneration,
}

pub struct ServerSideBuilder {
    src_path: PathBuf,
    dist_path: PathBuf,
    building_type: BuildingType,
    target_pages: Option<Vec<String>>,
}

impl ServerSideBuilder {
    pub fn new<S>(root: &S, dist_dir: &str, building_type: BuildingType) -> Result<Self>
    where
        S: AsRef<OsStr> + ?Sized,
    {
        let root = Path::new(root);
        let src_path = root.join("src");
        let dist_path = root.join(dist_dir);

        if !src_path.exists() {
            return Err(anyhow!("src directory not found."));
        }
        if !dist_path.exists() {
            fs::create_dir(dist_path.clone())?;
        }
        Ok(Self {
            src_path,
            dist_path,
            building_type,
            target_pages: None,
        })
    }

    pub fn with_target_pages(mut self, pages: Vec<String>) -> Self {
        self.target_pages = Some(pages);
        self
    }
}
// TODO: refactoring build function
impl Build for ServerSideBuilder {
    type Output = ();
    fn build(&self) -> Result<Self::Output> {
        let mut cache_dir = CacheDir::new(&format!("{}/cache", self.dist_path.display()))?;

        let src = SourceDir::new(&self.src_path).analyze()?;
        let all_pages = src.clone().pages;
        let pages = filter_target_pages(&self.target_pages, all_pages).unwrap();
        let (special_entries::App(app), special_entries::Head(head)) = src.specials()?;

        let targets = match TargetsGenerator::new(app, pages, &mut cache_dir).generate() {
            Ok(t) => t,
            Err(e) => return Err(anyhow!("Couldn't generate targets: {e}")),
        };

        let bundling_targets = targets.ready_for_bundling(&self.dist_path);
        let bundler = WebBundler::new(&bundling_targets, &self.dist_path)?;

        if let Err(e) = bundler.exec() {
            return Err(anyhow!("Bundling failed: {e}"));
        }

        let dist = DistDir::new(&self.dist_path)?.analyze()?;

        let manifest =
            ManifestGenerator::new(targets.clone(), cache_dir.clone(), dist).generate(&head)?;
        manifest.write(&self.dist_path.clone())?;

        if let Err(e) = HeadRenderer::new(&manifest.global.head, cache_dir.clone()).render(true) {
            return Err(anyhow!("Couldn't render head: {e}"));
        }

        if self.building_type == BuildingType::StaticSiteGeneration {
            if let Err(e) =
                PagesGenerator::new(targets, &head, &self.dist_path, cache_dir)?.generate()
            {
                return Err(anyhow!("Couldn't generate pages: {e}"));
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn scaffold_project(root: &Path) {
        let src = root.join("src");
        let pages = src.join("pages");
        fs::create_dir_all(&pages).unwrap();

        fs::write(
            src.join("_app.tsx"),
            "export default function App({ Component }) { return <Component /> }",
        )
        .unwrap();
        fs::write(
            src.join("_head.tsx"),
            "export default function Head() { return <title>Test</title> }",
        )
        .unwrap();
        fs::write(
            pages.join("index.tsx"),
            "export default function Index() { return <h1>Home</h1> }",
        )
        .unwrap();
    }

    fn scaffold_project_with_pages(root: &Path, pages: Vec<(&str, &str)>) {
        scaffold_project(root); // Creates base structure + default index.tsx

        let pages_dir = root.join("src/pages");
        for (name, content) in pages {
            let page_path = if name.contains('/') {
                let parts: Vec<_> = name.split('/').collect();
                let subdir = pages_dir.join(parts[0]);
                fs::create_dir_all(&subdir).unwrap();
                subdir.join(parts[1])
            } else {
                pages_dir.join(name)
            };
            fs::write(&page_path, content).unwrap();
        }
    }

    #[test]
    fn build_all_pages_by_default() {
        let tmp = TempDir::new().unwrap();
        scaffold_project_with_pages(
            tmp.path(),
            vec![
                (
                    "index.tsx",
                    "export default function Index() { return <h1>Home</h1> }",
                ),
                (
                    "about.tsx",
                    "export default function About() { return <p>About</p> }",
                ),
            ],
        );

        let builder =
            ServerSideBuilder::new(tmp.path(), "dist", BuildingType::ServerSideRendering).unwrap();
        let src = SourceDir::new(&builder.src_path).analyze().unwrap();
        let all_pages = src.clone().pages;

        let pages = filter_target_pages(&builder.target_pages, all_pages).unwrap();

        assert_eq!(pages.len(), 2, "expected 2 pages when no target filter");
    }

    #[test]
    fn build_only_target_pages() {
        let tmp = TempDir::new().unwrap();
        scaffold_project_with_pages(
            tmp.path(),
            vec![
                (
                    "index.tsx",
                    "export default function Index() { return <h1>Home</h1> }",
                ),
                (
                    "about.tsx",
                    "export default function About() { return <p>About</p> }",
                ),
            ],
        );

        let builder = ServerSideBuilder::new(tmp.path(), "dist", BuildingType::ServerSideRendering)
            .unwrap()
            .with_target_pages(vec!["index.tsx".to_string()]);

        let src = SourceDir::new(&builder.src_path).analyze().unwrap();
        let all_pages = src.clone().pages;

        let pages = filter_target_pages(&builder.target_pages, all_pages).unwrap();

        assert_eq!(pages.len(), 1, "expected 1 page when filtered");
        assert!(pages.contains_key("index.tsx"));
    }

    #[test]
    fn error_on_missing_target_page() {
        let tmp = TempDir::new().unwrap();
        scaffold_project_with_pages(
            tmp.path(),
            vec![(
                "index.tsx",
                "export default function Index() { return <h1>Home</h1> }",
            )],
        );

        let builder = ServerSideBuilder::new(tmp.path(), "dist", BuildingType::ServerSideRendering)
            .unwrap()
            .with_target_pages(vec!["nonexistent.tsx".to_string()]);

        let src = SourceDir::new(&builder.src_path).analyze().unwrap();
        let all_pages = src.clone().pages;

        let result = filter_target_pages(&builder.target_pages, all_pages);

        assert!(result.is_err(), "expected error for missing target page");
        let err_msg = result.unwrap_err().to_string();
        assert!(
            err_msg.contains("nonexistent.tsx"),
            "error should mention the missing page"
        );
    }
}
