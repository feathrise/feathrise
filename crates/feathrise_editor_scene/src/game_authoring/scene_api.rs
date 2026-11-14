//! 现有游戏作者适配实际使用的场景会话与UI出口；保留唯一状态和算法，不复制普通编辑器实现。

#[cfg(feature = "validation")]
pub mod validation;

pub mod resources {
    pub use crate::resources::{
        EditorAssetCatalog, SceneBrowserDragState, SceneBrowserState, SceneEditorLayoutState,
        SceneEditorNumericScrubMouseGuard, SceneEditorState, SceneEditorViewportState,
    };
}
pub mod components {
    // 实窗夹具只识别原恢复按钮身份；不提供直接改写加载事务或结果的入口。
    #[cfg(any(test, feature = "runtime_test_support"))]
    pub use crate::components::SceneLoadingRecoveryButton;
    pub use crate::components::{
        SceneEditorOwnedEntity, SceneEditorRightPanelScroll, SceneEditorViewport,
    };
}
pub mod types {
    pub use crate::types::{EditorTool, EditorView};
}
pub mod authoring {
    pub use crate::authoring::brush_path_dab_count;
    #[cfg(any(test, feature = "runtime_test_support"))]
    pub mod test_support {
        pub use crate::authoring::test_support::*;
    }
    pub use crate::authoring::AuthoringRetirement;
    pub use crate::authoring::domain_documents::*;
    pub use crate::authoring::{
        EditorAuthoringSession, EditorCommand, EditorSceneDraft, workspace_asset_root,
    };
    pub mod terrain_ground {
        pub use crate::authoring::terrain_ground::TerrainGroundSnapshot;
    }
    pub use crate::authoring::TerrainExternalChangeState;
    pub use crate::authoring::{TerrainLiveAdoption, TerrainLiveReceipt, TerrainLiveSet};
    #[cfg(any(test, feature = "runtime_test_support"))]
    pub use crate::authoring::terrain_live_test_support;
}
pub mod asset_catalog {
    pub use crate::asset_catalog::request_asset_reveal;
}
pub mod ui {
    pub mod vegetation_preset_editor {
        pub use crate::ui::vegetation_preset_editor::{
            PresetWorkspaceEditorHost, PresetWorkspaceTabHost,
        };
        pub use crate::ui::vegetation_preset_editor::{
            VegetationPresetEditorState, edit_shared_workspace,
        };
        pub use feathrise_editor_core::{PresetWorkspaceDynamicRoot, PresetWorkspaceTab};
    }
    pub mod sidebars {
        pub use crate::ui::sidebars::{
            EditorLeftSidebarTab, EditorRightSidebarTab, EditorSidebarTab, SceneEditorSidebarPage,
            SceneEditorSidebarState, SceneEditorSidebarTabButton, sidebar_context_title, sidebar_subsection_title,
            sidebar_tab_button, sidebar_tab_node, sidebar_tab_background, sidebar_tab_text_color,
            TOOL_MODULE_TAB_WIDTH, TOOL_MODULE_TAB_GAP,
        };
    }
    pub mod scene {
        pub use feathrise_editor_core::FONT_PATH;
    }
    pub mod text_input {
        pub use feathrise_editor_core::{EditorTextInput, EditorTextInputGuard};
    }
    pub mod theme {
        pub use feathrise_editor_ui::colors::*;
    }
    pub mod scene_context_menu {
        #[cfg(feature = "validation")]
        pub use crate::ui::scene_context_menu::queue_domain_action_for_validation;
        pub use crate::ui::scene_context_menu::{
            SceneContextMenuState, SceneMenuAction, SceneMenuDispatch,
        };
    }
    pub mod scene_inspector {
        pub use crate::ui::scene_inspector::InspectorSection;
    }
    pub mod asset_preview {
        pub use crate::ui::asset_preview::resources::AssetPreviewWorkspace;
        pub use crate::ui::asset_preview::types::AssetPreviewState;
        pub use feathrise_editor_core::AssetPreviewFocus;
        pub mod types {
            pub use crate::ui::asset_preview::types::{
                AssetPreviewContent, AssetPreviewModelBounds, AssetPreviewModelRoot, AssetPreviewRetiredRoot,
                AssetPreviewPhase, AssetPreviewState,
            };
        }
    }
}
pub mod viewport {
    pub use crate::viewport::{LayerCandidateState, SceneEditorBrushGizmos};
    pub mod modes {
        pub use crate::viewport::modes::ViewportModeState;
    }
    pub mod vegetation_ground {
        #[cfg(any(test, feature = "runtime_test_support"))]
        pub use crate::viewport::vegetation_ground::test_support;
        pub use crate::viewport::vegetation_ground::{GroundCursorTarget, VegetationGroundCursor};
    }
    pub mod snapping {
        pub use crate::viewport::snapping::{EditorGridGizmos, EditorGridOutlineGizmos};
    }
}
