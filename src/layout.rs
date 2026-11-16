use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::model::Link;

#[derive(Debug, Default, Clone, PartialEq, Serialize, Deserialize)]
pub struct LayoutFile {
    pub diagrams: BTreeMap<String, DiagramLayout>,
}

#[derive(Debug, Default, Clone, PartialEq, Serialize, Deserialize)]
pub struct DiagramLayout {
    #[serde(default)]
    pub positions: BTreeMap<String, [f32; 2]>,
    #[serde(default)]
    pub links: Vec<Link>,
}

pub fn path_for(source: &Path) -> PathBuf {
    let mut name = source.file_name().unwrap_or_default().to_os_string();
    name.push(".ferd");
    source.with_file_name(name)
}

pub fn load(source: &Path) -> Result<LayoutFile, String> {
    let path = path_for(source);
    match std::fs::read_to_string(&path) {
        Ok(text) => serde_json::from_str(&text).map_err(|e| format!("{}: {e}", path.display())),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(LayoutFile::default()),
        Err(e) => Err(format!("{}: {e}", path.display())),
    }
}

pub fn save(source: &Path, layout: &LayoutFile) -> Result<(), String> {
    let path = path_for(source);
    let text = serde_json::to_string_pretty(layout).map_err(|e| e.to_string())?;
    std::fs::write(&path, text + "\n").map_err(|e| format!("{}: {e}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Card;

    #[test]
    fn round_trip() {
        let dir = std::env::temp_dir().join(format!("ferd-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let source = dir.join("plan.md");
        assert_eq!(path_for(&source), dir.join("plan.md.ferd"));
        assert_eq!(load(&source).unwrap(), LayoutFile::default());

        let mut layout = LayoutFile::default();
        let d = layout.diagrams.entry("Plan".into()).or_default();
        d.positions.insert("entity_name".into(), [10.0, -20.5]);
        d.links.push(Link {
            from: "a".into(),
            to: "b".into(),
            label: "by hand".into(),
            from_card: Card::ExactlyOne,
            to_card: Card::ZeroOrMore,
        });
        save(&source, &layout).unwrap();
        assert_eq!(load(&source).unwrap(), layout);
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
