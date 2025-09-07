// src-tauri/src/commands/project.rs
//! Tauri commands for project management operations

use crate::commands::{CommandError, CommandResult, SuccessResponse};
use crate::project::{
    BuildOperation, BuildOutput, ProjectConfig, ProjectManager, ProjectStatistics, TemplateType,
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use tauri::{command, AppHandle, Manager, State};
use tokio::sync::RwLock;
use tracing::{debug, info, instrument};
use uuid::Uuid;

/// Global project manager state
pub type ProjectManagerState = Arc<RwLock<ProjectManager>>;

/// Request to open a project
#[derive(Debug, Deserialize)]
pub struct OpenProjectRequest {
    pub path: String,
}

/// Request to create a new project
#[derive(Debug, Deserialize)]
pub struct CreateProjectRequest {
    pub template_type: TemplateType,
    pub name: String,
    pub path: String,
    pub options: HashMap<String, String>,
}

/// Request to build a project
#[derive(Debug, Deserialize)]
pub struct BuildProjectRequest {
    pub operation: BuildOperation,
    pub targets: Option<Vec<String>>,
}

/// Project information for the frontend
#[derive(Debug, Serialize)]
pub struct ProjectInfo {
    pub id: String,
    pub name: String,
    pub root_path: String,
    pub project_type: crate::project::ProjectType,
    pub is_workspace: bool,
    pub workspace_members: Option<Vec<String>>,
    pub statistics: ProjectStatistics,
    pub last_modified: Option<u64>,
}

/// Build status response
#[derive(Debug, Serialize)]
pub struct BuildStatusResponse {
    pub status: crate::project::BuildStatus,
    pub output: Option<BuildOutput>,
}

/// File tree response
#[derive(Debug, Serialize)]
pub struct FileTreeResponse {
    pub root: crate::project::FileNode,
    pub statistics: crate::project::FileTreeStats,
}

/// Open a project
#[command]
#[instrument(skip(project_manager, request))]
pub async fn open_project(
    project_manager: State<'_, ProjectManagerState>,
    request: OpenProjectRequest,
) -> CommandResult<ProjectInfo> {
    let path = PathBuf::from(&request.path);
    
    let manager = project_manager.read().await;
    let project_id = manager.open_project(path).await.map_err(|e| {
        CommandError::OperationFailed {
            message: format!("Failed to open project: {}", e),
        }
    })?;

    let project = manager.get_project(project_id).await.map_err(|e| {
        CommandError::OperationFailed {
            message: format!("Failed to get project: {}", e),
        }
    })?;

    let workspace_members = project.workspace_members().map(|members| {
        members.iter().map(|m| m.name.clone()).collect()
    });

    let last_modified = project.last_modified
        .duration_since(std::time::UNIX_EPOCH)
        .ok()
        .map(|d| d.as_secs());

    Ok(ProjectInfo {
        id: project.id.to_string(),
        name: project.name.clone(),
        root_path: project.root_path.to_string_lossy().to_string(),
        project_type: project.project_type,
        is_workspace: project.is_workspace(),
        workspace_members,
        statistics: project.statistics(),
        last_modified,
    })
}

/// Close a project
#[command]
#[instrument(skip(project_manager))]
pub async fn close_project(
    project_manager: State<'_, ProjectManagerState>,
    project_id: String,
) -> CommandResult<SuccessResponse> {
    let id = Uuid::parse_str(&project_id).map_err(|_| CommandError::InvalidParameter {
        parameter: "project_id".to_string(),
    })?;

    let manager = project_manager.read().await;
    manager.close_project(id).await.map_err(|e| {
        CommandError::OperationFailed {
            message: format!("Failed to close project: {}", e),
        }
    })?;

    info!("Closed project: {}", project_id);
    Ok(SuccessResponse::new("Project closed successfully"))
}

/// Get all open projects
#[command]
#[instrument(skip(project_manager))]
pub async fn list_projects(
    project_manager: State<'_, ProjectManagerState>,
) -> CommandResult<Vec<ProjectInfo>> {
    let manager = project_manager.read().await;
    let projects = manager.list_projects().await;

    let project_infos = projects
        .into_iter()
        .map(|project| {
            let workspace_members = project.workspace_members().map(|members| {
                members.iter().map(|m| m.name.clone()).collect()
            });

            let last_modified = project.last_modified
                .duration_since(std::time::UNIX_EPOCH)
                .ok()
                .map(|d| d.as_secs());

            ProjectInfo {
                id: project.id.to_string(),
                name: project.name,
                root_path: project.root_path.to_string_lossy().to_string(),
                project_type: project.project_type,
                is_workspace: project.is_workspace(),
                workspace_members,
                statistics: project.statistics(),
                last_modified,
            }
        })
        .collect();

    Ok(project_infos)
}

/// Create a new project from template
#[command]
#[instrument(skip(project_manager, request))]
pub async fn create_project(
    project_manager: State<'_, ProjectManagerState>,
    request: CreateProjectRequest,
) -> CommandResult<ProjectInfo> {
    let target_path = PathBuf::from(&request.path);

    let manager = project_manager.read().await;
    let project_id = manager
        .create_project(
            request.template_type,
            request.name,
            target_path,
            request.options,
        )
        .await
        .map_err(|e| CommandError::OperationFailed {
            message: format!("Failed to create project: {}", e),
        })?;

    let project = manager.get_project(project_id).await.map_err(|e| {
        CommandError::OperationFailed {
            message: format!("Failed to get created project: {}", e),
        }
    })?;

    let workspace_members = project.workspace_members().map(|members| {
        members.iter().map(|m| m.name.clone()).collect()
    });

    let last_modified = project.last_modified
        .duration_since(std::time::UNIX_EPOCH)
        .ok()
        .map(|d| d.as_secs());

    info!("Created new project: {}", project.name);

    Ok(ProjectInfo {
        id: project.id.to_string(),
        name: project.name.clone(),
        root_path: project.root_path.to_string_lossy().to_string(),
        project_type: project.project_type,
        is_workspace: project.is_workspace(),
        workspace_members,
        statistics: project.statistics(),
        last_modified,
    })
}

/// Get project file tree
#[command]
#[instrument(skip(project_manager))]
pub async fn get_project_file_tree(
    project_manager: State<'_, ProjectManagerState>,
    project_id: String,
) -> CommandResult<FileTreeResponse> {
    let id = Uuid::parse_str(&project_id).map_err(|_| CommandError::InvalidParameter {
        parameter: "project_id".to_string(),
    })?;

    let manager = project_manager.read().await;
    let project = manager.get_project(id).await.map_err(|e| {
        CommandError::OperationFailed {
            message: format!("Failed to get project: {}", e),
        }
    })?;

    let root = project
        .file_tree
        .root_node()
        .ok_or_else(|| CommandError::OperationFailed {
            message: "File tree not available".to_string(),
        })?
        .clone();

    let statistics = project.file_tree.statistics();

    Ok(FileTreeResponse { root, statistics })
}

/// Build a project
#[command]
#[instrument(skip(project_manager, request))]
pub async fn build_project(
    project_manager: State<'_, ProjectManagerState>,
    project_id: String,
    request: BuildProjectRequest,
) -> CommandResult<BuildOutput> {
    let id = Uuid::parse_str(&project_id).map_err(|_| CommandError::InvalidParameter {
        parameter: "project_id".to_string(),
    })?;

    let manager = project_manager.write().await;
    let projects_map = manager.projects_map();
    let mut projects = projects_map.write().await;

    if let Some(project) = projects.get_mut(&id) {
        let output = project
            .build_manager
            .execute_build(request.operation, request.targets)
            .await
            .map_err(|e| CommandError::OperationFailed {
                message: format!("Build failed: {}", e),
            })?;

        info!(
            "Build completed for project: {} with status: {:?}",
            project.name, output.status
        );

        Ok(output)
    } else {
        Err(CommandError::OperationFailed {
            message: format!("Project not found: {}", project_id),
        })
    }
}

/// Get available project templates
#[command]
pub async fn get_project_templates() -> CommandResult<Vec<crate::project::templates::ProjectTemplate>> {
    let config = crate::project::TemplateConfig::default();
    let engine = crate::project::TemplateEngine::new(config);
    
    let templates = engine
        .available_templates()
        .into_iter()
        .map(|(_, template)| template.clone())
        .collect();

    Ok(templates)
}

/// Initialize project management system
pub fn init_project_commands(app: &AppHandle) -> crate::project::ProjectManager {
    let config = ProjectConfig::default();
    let project_manager = crate::project::init_project_system(config);

    // Store the project manager in Tauri's state
    app.manage(ProjectManagerState::new(RwLock::new(project_manager.clone())));

    info!("Project management system initialized");
    project_manager
}