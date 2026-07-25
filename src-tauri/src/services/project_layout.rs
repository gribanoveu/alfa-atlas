use crate::domain::layout::PanelLayout;
use crate::domain::settings::SettingsError;
use crate::infra::layout_store;

pub fn load_layout(project_root: &str) -> Result<PanelLayout, SettingsError> {
    layout_store::load(project_root)
}

pub fn save_layout(project_root: &str, layout: PanelLayout) -> Result<(), SettingsError> {
    layout_store::save(project_root, &layout.clamped())
}
