use crate::utils::{UtilError, UtilResult};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::Arc;
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::{Child, Command};
use tokio::sync::{mpsc, RwLock};
use tokio::time::timeout;
use tracing::{debug, error, info, instrument, warn};
use uuid::Uuid;

/// Error types for process operations
pub enum ProcessError {
    #[error("Process spawn failed: {message}")]
    SpawnFailed { message: String },

    #[error("Process not found: {id}")]
    NotFound { id: String },

    #[error("Process timeout after {duration:?}")]
    Timeout { duration: Duration },

    #[error("Process killed")]
    Killed,

    #[error("Process exited with code {code}")]
    ExitCode { code: i32 },

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
}

/// Command output with streams
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommandOutput {
    pub exit_code: Option<i32>,
    pub stdout: String,
    pub stderr: String,
    pub execution_time: Duration,
}

/// Process information
#[derive(Debug, Clone)]
pub struct ProcessInfo {
    pub id: Uuid,
    pub command: String,
    pub args: Vec<String>,
    pub working_dir: Option<PathBuf>,
    pub started_at: std::time::Instant,
    pub pid: Option<u32>,
}

/// Process event types
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type")]
pub enum ProcessEvent {
    Started { id: String, pid: u32 },
    StdoutLine { id: String, line: String },
    StderrLine { id: String, line: String },
    Exited { id: String, exit_code: Option<i32> },
    Error { id: String, error: String },
}

/// Configuration for process execution
#[derive(Debug, Clone)]
pub struct ProcessConfig {
    pub working_dir: Option<PathBuf>,
    pub environment: HashMap<String, String>,
    pub timeout: Option<Duration>,
    pub capture_output: bool,
    pub inherit_env: bool,
}

impl Default for ProcessConfig {
    fn default() -> Self {
        Self {
            working_dir: None,
            environment: HashMap::new(),
            timeout: Some(Duration::from_secs(30)),
            capture_output: true,
            inherit_env: true,
        }
    }
}
/// Manages running processes
pub struct ProcessManager {
    processes: Arc<RwLock<HashMap<Uuid, (ProcessInfo, Child)>>>,
    event_sender: mpsc::UnboundedSender<ProcessEvent>,
}

impl ProcessManager {
    /// Create a new process manager
    pub fn new() -> (Self, mpsc::UnboundedReceiver<ProcessEvent>) {
        let (event_sender, event_receiver) = mpsc::unbounded_channel();

        let manager = Self {
            processes: Arc::new(RwLock::new(HashMap::new())),
            event_sender,
        };

        (manager, event_receiver)
    }

    /// Spawn a new process
    #[instrument(skip(self, config))]
    pub async fn spawn_process(
        &self,
        command: &str,
        args: &[String],
        config: ProcessConfig,
    ) -> UtilResult<Uuid> {
        let process_id = Uuid::new_v4();

        let mut cmd = Command::new(command);
        cmd.args(args);

        if let Some(working_dir) = &config.working_dir {
            cmd.current_dir(working_dir);
        }

        if config.inherit_env {
            cmd.env_clear();
            for (key, value) in std::env::vars() {
                cmd.env(key, value);
            }
        }

        for (key, value) in &config.environment {
            cmd.env(key, value);
        }

        if config.capture_output {
            cmd.stdout(Stdio::piped());
            cmd.stderr(Stdio::piped());
        }

        let mut child = cmd.spawn().map_err(|e| ProcessError::SpawnFailed {
            message: format!("Failed to spawn {}: {}", command, e),
        })?;

        let pid = child.id();

        let process_info = ProcessInfo {
            id: process_id,
            command: command.to_string(),
            args: args.to_vec(),
            working_dir: config.working_dir.clone(),
            started_at: std::time::Instant::now(),
            pid,
        };

        self.processes
            .write()
            .await
            .insert(process_id, (process_info.clone(), child));

        if let Some(pid) = pid {
            let _ = self.event_sender.send(ProcessEvent::Started {
                id: process_id.to_string(),
                pid,
            });
        }

        info!(
            "Spawned process: {} {} (ID: {}, PID: {:?})",
            command,
            args.join(" "),
            process_id,
            pid,
        );

        Ok(process_id)
    }

    /// Execute a command and wait for completion
    #[instrument(skip(self, config))]
    pub async fn execute_command(
        &self,
        command: &str,
        args: &[String],
        config: ProcessConfig,
    ) -> UtilResult<CommandOutput> {
        let start_time = std::time::Instant::now();

        let mut cmd = Command::new(command);
        cmd.args(args);

        if let Some(working_dir) = &config.working_dir {
            cmd.current_dir(working_dir);
        }

        if config.inherit_env {
            for (key, value) in std::env::vars() {
                cmd.env(key, value);
            }
        }

        for (key, value) in &config.environment {
            cmd.env(key, value);
        }

        cmd.stdout(Stdio::piped());
        cmd.stderr(Stdio::piped());

        let execute_future = async {
            let output = cmd.output().await.map_err(|e| ProcessError::SpawnFailed {
                message: format!("Failed to execute {}: {}", command, e),
            })?;

            let stdout = String::from_utf8_lossy(&output.stdout).to_string();
            let stderr = String::from_utf8_lossy(&output.stderr).to_string();

            let result = CommandOutput {
                exit_code: output.status.code(),
                stdout,
                stderr,
                execution_time: start_time.elapsed(),
            };

            Ok::<CommandOutput, ProcessError>(result)
        };

        let output = if let Some(timeout_duration) = config.timeout {
            timeout(timeout_duration, execute_future)
                .await
                .map_err(|_| ProcessError::Timeout {
                    duration: timeout_duration,
                })??
        } else {
            execute_future.await?
        };

        debug!(
            "Executed command: {} {} (exit code: {:?}, duration: {:?})",
            command,
            args.join(" ")
            output.exit_code,
            output.execution_time
        );

        Ok(output)
    }

    /// Kill a running process
    #[instrument(skip(self))]
    pub async fn kill_process(&self, process_id: Uuid) -> UtilResult<()> {
        let mut processes = self.processes.write().await;

        if let Some((info, mut child)) = processes.remove(&process_id) {
            let result = child.kill().await;

            match result {
                Ok(()) => {
                    info!("Killed process: {} (ID: {})", info.command, process_id);
                }
                Err(e) => {
                    warn!("Failed to kill process {}: {}", process_id, e);
                }
            }

            let _ = self.event_sender.send(ProcessEvent::Error {
                id: process_id.to_string(),
                error: "Process killed".to_string(),
            });

            Ok(())
        } else {
            Err(UtilError::Process(ProcessError::NotFound {
                id: process_id.to_string(),
            }))
        }
    }

    /// Get information about a running process
    pub async fn get_process_info(&self, process_id: Uuid) -> Option<ProcessInfo> {
        let processes = self.processes.read().await;
        processes.get(&process_id).map(|(info, _)| info.clone())
    }

    /// List all running processes
    pub async fn list_processes(&self) -> Vec<ProcessInfo> {
        let processes = self.processes.read().await;
        processes.values().map(|(info, _)| info.clone()).collect()
    }

    /// Wait for a process to complete
    #[instrument(skip(self))]
    pub async fn wait_for_process(&self, process_id: Uuid) -> UtilResult<CommandOutput> {
        let start_time = std::time::Instant::now();

        // Get and remove the process from our tracking
        let (info, mut child) = {
            let mut processes = self.processes.write().await;
            processes.remove(&process_id).ok_or_else(|| {
                UtilError::Process(ProcessError::NotFound {
                    id: process_id.to_string(),
                })
            })?
        };

        // Capture output if available
        let mut stdout = String::new();
        let mut stderr = String::new();

        if let Some(stdout_pipe) = child.stdout.take() {
            let mut reader = BufReader::new(stdout_pipe);
            let mut line = String::new();

            while reader.read_line(&mut line).await? > 0 {
                let trimmed_line = line.trim_end().to_string();
                stdout.push_str(&line);

                let _ = self.event_sender.send(ProcessEvent::StdoutLine {
                    id: process_id.to_string(),
                    line: trimmed_line,
                });

                line.clear();
            }
        }

        if let Some(stderr_pipe) = child.stderr.take() {
            let mut reader = BufReader::new(stderr_pipe);
            let mut line = String::new();

            while reader.read_line(&mut line).await? > 0 {
                let trimmed_line = line.trim_end().to_string();
                stderr.push_str(&line);

                let _ = self.event_sender.send(ProcessEvent::StderrLine {
                    id: process_id.to_string(),
                    line: trimmed_line,
                });

                line.clear();
            }
        }

        // Wait for process to complete
        let exit_status = child.wait().await?;
        let exit_code = exit_status.code();

        let output = CommandOutput {
            exit_code,
            stdout,
            stderr,
            execution_time: start_time.elapsed(),
        };

        let _ = self.event_sender.send(ProcessEvent::Exited {
            id: process_id.to_string(),
            exit_code,
        });

        info!(
            "Process completed: {} (ID: {}, exit code: {:?})",
            info.command, process_id, exit_code
        );

        Ok(output)
    }

    /// Kill all running processes
    pub async fn kill_all_processes(&self) -> UtilResult<()> {
        let process_ids: Vec<Uuid> = {
            let processes = self.processes.read().await;
            processes.keys().copied().collect()
        };

        for process_id in process_ids {
            if let Err(e) = self.kill_process(process_id).await {
                warn!("Failed to kill process {}: {}", process_id, e);
            }
        }

        Ok(())
    }
}

impl Default for ProcessManager {
    fn default() -> Self {
        let (manager, _) = Self::new();
        manager
    }
}

static PROCESS_MANAGER: once_cell::sync::OnceCell<ProcessManager> =
    once_cell::sync::OnceCell::new();

/// Initialize global process manager
pub fn init_process_manager() -> mpsc::UnboundedReceiver<ProcessEvent> {
    let (manager, receiver) = ProcessManager::new();

    if PROCESS_MANAGER.set(manager).is_err() {
        panic!("Process manager already initialized");
    }

    receiver
}

/// Get global process manager
pub fn get_process_manager() -> &'static ProcessManager {
    PROCESS_MANAGER
        .get()
        .expect("Process manager not initialized")
}

/// Shutdown all processes
pub async fn shutdown_processes() -> UtilResult<()> {
    if let Some(manager) = PROCESS_MANAGER.get() {
        manager.kill_all_processes().await?;
    }
    Ok(())
}

/// Convenience function to execute a simple command
pub async fn execute_simple_command(command: &str, args: &[&str]) -> UtilResult<CommandOutput> {
    let manager = get_process_manager();
    let args: Vec<String> = args.iter().map(|&s| s.to_string()).collect();
    let config = ProcessConfig::default();

    manager.execute_command(command, &args, config).await
}

/// Execute command in specific directory
pub async fn execute_command_in_dir<P: Into<PathBuf>>(
    command: &str,
    args: &[&str],
    working_dir: P,
) -> UtilResult<CommandOutput> {
    let manager = get_process_manager();
    let args: Vec<String> = args.iter().map(|&s| s.to_string()).collect();
    let config = ProcessConfig {
        working_dir: Some(working_dir.into()),
        ..ProcessConfig::default()
    };

    manager.execute_command(command, &args, config).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[tokio::test]
    async fn test_execute_simple_command() {
        let (manager, _) = ProcessManager::new();

        let output = manager
            .execute_command("echo", &["hello".to_string()], ProcessConfig::default())
            .await
            .unwrap();

        assert_eq!(output.exit_code, Some(0));
        assert_eq!(output.stdout.trim(), "hello");
    }

    #[tokio::test]
    async fn test_process_spawn_and_kill() {
        let (manager, mut events) = ProcessManager::new();

        // Spawn a long-running process
        let process_id = manager
            .spawn_process("sleep", &["5".to_string()], ProcessConfig::default())
            .await
            .unwrap();

        // Verify it's running
        let info = manager.get_process_info(process_id).await.unwrap();
        assert_eq!(info.command, "sleep");

        // Kill it
        manager.kill_process(process_id).await.unwrap();

        // Check that we can't find it anymore
        assert!(manager.get_process_info(process_id).await.is_none());
    }

    #[tokio::test]
    async fn test_command_timeout() {
        let (manager, _) = ProcessManager::new();

        let config = ProcessConfig {
            timeout: Some(Duration::from_millis(100)),
            ..ProcessConfig::default()
        };

        let result = manager
            .execute_command("sleep", &["1".to_string()], config)
            .await;

        assert!(matches!(
            result,
            Err(UtilError::Process(ProcessError::Timeout { .. }))
        ));
    }
}
