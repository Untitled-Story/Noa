use super::AssetIndex;
use crate::{convert::unity_color_space, diagnostics::Error, unity::UnityFile};
use noa_format::ir::{ColorContext, ColorSpace};
use std::fs;

/// Source-project settings, not output/display settings or gradient key metadata.
/// Additional dependency roots cannot override the entry project's environment.
pub fn resolve_color_context(index: &AssetIndex) -> Result<ColorContext, Error> {
    let root = index
        .roots
        .first()
        .ok_or_else(|| Error::new("INVALID_SEARCH_ROOT", "asset index has no primary root"))?;
    let settings = root
        .parent()
        .map(|p| p.join("ProjectSettings/ProjectSettings.asset"))
        .filter(|p| root.file_name().is_some_and(|n| n == "Assets") && p.is_file());
    let project_color_space = if let Some(settings) = settings {
        let text = fs::read_to_string(&settings).map_err(|e| Error::io(&settings, e))?;
        let file = UnityFile::parse(&text)?;
        let mut objects = file
            .objects
            .values()
            .filter(|o| o.class_id == 129 && o.type_name == "PlayerSettings");
        let object = objects.next().ok_or_else(|| {
            Error::new(
                "INVALID_PROJECT_COLOR_SPACE",
                "ProjectSettings/ProjectSettings.asset: missing PlayerSettings",
            )
        })?;
        if objects.next().is_some() {
            return Err(Error::new(
                "INVALID_PROJECT_COLOR_SPACE",
                "multiple PlayerSettings objects",
            ));
        }
        match object.data.get("m_ActiveColorSpace") {
            None => ColorSpace::Unspecified,
            Some(v) => unity_color_space(
                v.as_i64().ok_or_else(|| {
                    Error::new(
                        "INVALID_PROJECT_COLOR_SPACE",
                        "PlayerSettings.m_ActiveColorSpace must be an integer",
                    )
                })?,
                "PlayerSettings.m_ActiveColorSpace",
            )?,
        }
    } else {
        ColorSpace::Unspecified
    };
    Ok(ColorContext {
        project_color_space,
        host_project_color_space_slot: (project_color_space == ColorSpace::Unspecified)
            .then(|| "source-project-color-space".into()),
    })
}
