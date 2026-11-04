//! 地形工具页的固定导航、常驻正文与反馈标记，不承担作者数据身份。

use bevy::prelude::*;

use super::resources::TerrainSidebarPage;
use crate::types::EditorTool;

#[derive(Component, Clone, Default)]
pub(super) struct TerrainSidebarRoot;

#[derive(Component, Clone, Default)]
pub(super) struct TerrainSidebarMeshContext;

#[derive(Component, Clone, Default)]
pub(super) struct TerrainSidebarScroll;

#[derive(Component, Clone, Default)]
pub(super) struct TerrainSidebarTab(pub(super) TerrainSidebarPage);

#[derive(Component, Clone, Default)]
pub(super) struct TerrainSidebarPanel(pub(super) TerrainSidebarPage);

#[derive(Component, Clone, Default)]
pub(super) struct TerrainSidebarTool(pub(super) EditorTool);

#[derive(Component, Clone, Default)]
pub(super) struct TerrainSidebarModeText;

#[derive(Component, Clone, Default)]
pub(super) struct TerrainSidebarHintText;

#[derive(Component, Clone, Default)]
pub(super) struct TerrainSidebarAvailabilityText;

#[derive(Component, Clone, Default)]
pub(super) struct TerrainSidebarMessageText;

#[derive(Component, Clone, Default)]
pub(super) struct TerrainSidebarPuddleTargetText;

#[derive(Component, Clone, Default)]
pub(super) struct VolumeBrushControls;

#[derive(Component, Clone, Default)]
pub(super) struct VolumeBrushTypeRow;

#[derive(Component, Clone, Default)]
pub(super) struct VolumeBrushShapeButton(pub(super) feathrise_map_terrain::volume::VolumeBrushShape);

#[derive(Component, Clone, Default)]
pub(super) struct VolumeBrushHeightRow;

#[derive(Component, Clone, Default)]
pub(super) struct VolumeBrushHeightSlot;

#[derive(Component)]
pub(super) struct VolumeBrushHeightField;

#[derive(Component, Clone, Default)]
pub(super) struct VolumeBrushHintText;

#[derive(Component, Clone, Default)]
pub(super) struct VolumeBrushHeightLockButton;
