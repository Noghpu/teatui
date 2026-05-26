#![cfg(windows)]
// Windows-only fake-service integration tests. The fake `jj`/`tea` shims are
// `.cmd` batch scripts driven by `powershell`, so the file compiles to an
// empty crate on other platforms. See AGENTS.md for the `windows_` test
// convention.

use std::fs;
use std::io;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use teatui::context::{self, ContextBundle};
use teatui::event::BackgroundEvent;
use teatui::generate::{
    ExecutionPlan, FieldState, GeneratePhase, PrForm, RevsetSummary, StaleCheckResult,
};
use teatui::jj;
use teatui::llm::LlmClient;
use teatui::repo::{
    BaseBranchInfo, BaseBranchSource, LlmBackendStatus, LlmStatus, RemoteInfo, RepoState, TeaAuth,
    ToolStatus,
};
use teatui::{config::Config, prompt};
use tempfile::TempDir;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{Mutex as AsyncMutex, mpsc, oneshot};
use tokio::time::timeout;

// Serializes tests so that the global FAKE_* env vars set by `set_fake_env`
// don't collide across cargo's parallel test runner.
static TEST_LOCK: OnceLock<AsyncMutex<()>> = OnceLock::new();

async fn test_lock() -> tokio::sync::MutexGuard<'static, ()> {
    TEST_LOCK.get_or_init(|| AsyncMutex::new(())).lock().await
}

#[derive(Debug, Clone)]
struct RecordedRequest {
    request_line: String,
    headers: Vec<(String, String)>,
    body: String,
}

#[derive(Debug)]
struct FakeOllamaServer {
    base_url: String,
    requests: Arc<Mutex<Vec<RecordedRequest>>>,
    shutdown: Option<oneshot::Sender<()>>,
}

impl FakeOllamaServer {
    async fn start(response: String) -> io::Result<Self> {
        let listener = TcpListener::bind("127.0.0.1:0").await?;
        let addr = listener.local_addr()?;
        let requests = Arc::new(Mutex::new(Vec::new()));
        let requests_clone = Arc::clone(&requests);
        let (shutdown_tx, mut shutdown_rx) = oneshot::channel();

        tokio::spawn(async move {
            loop {
                tokio::select! {
                    biased;
                    _ = &mut shutdown_rx => break,
                    accept = listener.accept() => {
                        let Ok((stream, _peer)) = accept else {
                            break;
                        };
                        let requests = Arc::clone(&requests_clone);
                        let response = response.clone();
                        tokio::spawn(async move {
                            let _ = handle_fake_ollama_connection(stream, requests, response).await;
                        });
                    }
                }
            }
        });

        Ok(Self {
            base_url: format!("http://{}", addr),
            requests,
            shutdown: Some(shutdown_tx),
        })
    }

    fn base_url(&self) -> &str {
        &self.base_url
    }

    fn requests(&self) -> Vec<RecordedRequest> {
        self.requests.lock().expect("requests poisoned").clone()
    }
}

impl Drop for FakeOllamaServer {
    fn drop(&mut self) {
        if let Some(shutdown) = self.shutdown.take() {
            let _ = shutdown.send(());
        }
    }
}

async fn handle_fake_ollama_connection(
    mut stream: TcpStream,
    requests: Arc<Mutex<Vec<RecordedRequest>>>,
    response: String,
) -> io::Result<()> {
    let mut buffer = Vec::new();
    let mut header_end = None;
    let mut temp = [0_u8; 1024];

    loop {
        let read = stream.read(&mut temp).await?;
        if read == 0 {
            break;
        }
        buffer.extend_from_slice(&temp[..read]);
        if let Some(index) = find_header_end(&buffer) {
            header_end = Some(index);
            break;
        }
    }

    let Some(header_end) = header_end else {
        return Ok(());
    };
    let headers_raw = String::from_utf8_lossy(&buffer[..header_end]).into_owned();
    let content_length = headers_raw
        .lines()
        .skip(1)
        .filter_map(|line| {
            line.split_once(':')
                .map(|(name, value)| (name.trim(), value.trim()))
        })
        .find(|(name, _)| name.eq_ignore_ascii_case("content-length"))
        .and_then(|(_, value)| value.parse::<usize>().ok())
        .unwrap_or(0);

    let body_start = header_end + 4;
    let mut body = buffer[body_start..].to_vec();
    while body.len() < content_length {
        let read = stream.read(&mut temp).await?;
        if read == 0 {
            break;
        }
        body.extend_from_slice(&temp[..read]);
    }

    let request_line = headers_raw.lines().next().unwrap_or_default().to_string();
    let headers = headers_raw
        .lines()
        .skip(1)
        .filter_map(|line| {
            line.split_once(':')
                .map(|(name, value)| (name.trim().to_string(), value.trim().to_string()))
        })
        .collect::<Vec<_>>();
    let body = String::from_utf8_lossy(&body[..content_length.min(body.len())]).into_owned();
    requests
        .lock()
        .expect("requests poisoned")
        .push(RecordedRequest {
            request_line,
            headers,
            body,
        });

    let response = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        response.len(),
        response
    );
    stream.write_all(response.as_bytes()).await?;
    stream.shutdown().await?;
    Ok(())
}

fn find_header_end(buffer: &[u8]) -> Option<usize> {
    buffer.windows(4).position(|window| window == b"\r\n\r\n")
}

struct FakeCommandTree {
    _root: TempDir,
    workspace: PathBuf,
    jj: PathBuf,
    tea: PathBuf,
    jj_log: PathBuf,
    tea_log: PathBuf,
}

impl FakeCommandTree {
    fn new() -> io::Result<Self> {
        let root = TempDir::new()?;
        let bin_dir = root.path().join("bin");
        let workspace = root.path().join("workspace");
        fs::create_dir_all(&bin_dir)?;
        fs::create_dir_all(&workspace)?;

        let jj_log = root.path().join("jj-argv.log");
        let tea_log = root.path().join("tea-argv.log");
        let jj = bin_dir.join("jj.cmd");
        let tea = bin_dir.join("tea.cmd");
        fs::write(&jj, jj_script())?;
        fs::write(&tea, tea_script())?;

        Ok(Self {
            _root: root,
            workspace,
            jj,
            tea,
            jj_log,
            tea_log,
        })
    }

    fn config(&self, base_url: &str) -> Config {
        let mut config = Config::default();
        config.commands.jj = self.jj.display().to_string();
        config.commands.tea = self.tea.display().to_string();
        config.commands.git = "git".into();
        config.llm.backends[0].base_url = base_url.into();
        config
    }

    fn repo_state(&self, base_url: &str) -> RepoState {
        RepoState {
            workspace_root: Some(self.workspace.clone()),
            inside_workspace: true,
            jj: ToolStatus::Available,
            git: ToolStatus::Available,
            tea: ToolStatus::Available,
            tea_auth: TeaAuth::Configured {
                host: "code.example.com".into(),
                user: Some("alice".into()),
            },
            remote: Some(RemoteInfo::parse("git@code.example.com:team/project.git")),
            base_branch: BaseBranchInfo {
                name: "main".into(),
                source: BaseBranchSource::Config,
            },
            llm_active: "default".into(),
            llm_backends: vec![LlmBackendStatus {
                name: "default".into(),
                backend_type: "ollama".into(),
                base_url: base_url.into(),
                model: "qwen3".into(),
                status: LlmStatus::Reachable,
            }],
            blockers: Vec::new(),
        }
    }

    fn write_jj_outputs(
        &self,
        root: &str,
        status: &str,
        log: &str,
        diff_stats: &str,
        diff: &str,
    ) -> io::Result<()> {
        fs::write(self.workspace.join("jj-root.txt"), root)?;
        fs::write(self.workspace.join("jj-status.txt"), status)?;
        fs::write(self.workspace.join("jj-log.txt"), log)?;
        fs::write(self.workspace.join("jj-diff-stats.txt"), diff_stats)?;
        fs::write(self.workspace.join("jj-diff.txt"), diff)?;
        Ok(())
    }

    fn write_tea_outputs(&self, login_list: &str, pr_url: &str) -> io::Result<()> {
        fs::write(self.workspace.join("tea-login-list.txt"), login_list)?;
        fs::write(self.workspace.join("tea-pr-url.txt"), pr_url)?;
        Ok(())
    }
}

fn jj_script() -> String {
    [
        "@echo off",
        "setlocal enabledelayedexpansion",
        "if not defined FAKE_JJ_LOG exit /b 2",
        ">>\"%FAKE_JJ_LOG%\" echo %*",
        "if \"%1\"==\"--version\" exit /b 0",
        "if \"%1\"==\"--no-pager\" goto no_pager",
        "echo unsupported jj command 1>&2",
        "exit /b 1",
        ":no_pager",
        "if \"%2\"==\"root\" goto root",
        "if \"%2\"==\"status\" goto status",
        "if \"%2\"==\"log\" goto log",
        "if \"%2\"==\"diff\" goto diff",
        "if \"%2\"==\"bookmark\" goto bookmark",
        "if \"%2\"==\"git\" goto git",
        "echo unsupported jj subcommand 1>&2",
        "exit /b 1",
        ":root",
        "type \"%FAKE_JJ_ROOT_FILE%\"",
        "exit /b 0",
        ":status",
        "type \"%FAKE_JJ_STATUS_FILE%\"",
        "exit /b 0",
        ":log",
        "type \"%FAKE_JJ_LOG_FILE%\"",
        "exit /b 0",
        ":diff",
        "if \"%5\"==\"--stat\" goto diff_stats",
        "type \"%FAKE_JJ_DIFF_FILE%\"",
        "exit /b 0",
        ":diff_stats",
        "type \"%FAKE_JJ_DIFF_STATS_FILE%\"",
        "exit /b 0",
        ":bookmark",
        "exit /b 0",
        ":git",
        "if \"%3\"==\"push\" exit /b 0",
        "echo unsupported jj git subcommand 1>&2",
        "exit /b 1",
    ]
    .join("\r\n")
}

fn tea_script() -> String {
    [
        "@echo off",
        "setlocal enabledelayedexpansion",
        "if not defined FAKE_TEA_LOG exit /b 2",
        ">>\"%FAKE_TEA_LOG%\" echo %*",
        "if \"%1\"==\"--version\" exit /b 0",
        "if \"%1\"==\"login\" goto login",
        "if \"%1\"==\"pr\" goto pr",
        "echo unsupported tea command 1>&2",
        "exit /b 1",
        ":login",
        "type \"%FAKE_TEA_LOGIN_LIST_FILE%\"",
        "exit /b 0",
        ":pr",
        "if not \"%2\"==\"create\" exit /b 1",
        "powershell -NoLogo -NonInteractive -Command \"$ErrorActionPreference='Stop'; Write-Output 'creating PR'; Write-Output $env:FAKE_TEA_PR_URL; Write-Output 'thanks'; if ($env:FAKE_TEA_FORCE_FAIL) { exit 17 }\"",
        "exit /b %ERRORLEVEL%",
    ]
    .join("\r\n")
}

fn sample_revset(bookmarks: Vec<String>) -> RevsetSummary {
    RevsetSummary::new(
        "@",
        "Keep the current change",
        bookmarks,
        "1 file changed, 2 insertions(+), 1 deletion(-)",
        1,
        vec!["abc123".into()],
        vec!["def456".into()],
        vec!["abc123 def456 Keep the current change".into()],
        vec!["watch for stale context".into()],
    )
}

fn sample_form() -> PrForm {
    let mut form = PrForm::new("@", "feature/example", "main");
    form.title = FieldState::new("Improve PR generation");
    form.description = FieldState::new("Summary\n\nTesting\n\nRisks");
    form
}

fn set_fake_env(tree: &FakeCommandTree) {
    unsafe {
        std::env::set_var("FAKE_JJ_LOG", &tree.jj_log);
        std::env::set_var("FAKE_JJ_ROOT_FILE", tree.workspace.join("jj-root.txt"));
        std::env::set_var("FAKE_JJ_STATUS_FILE", tree.workspace.join("jj-status.txt"));
        std::env::set_var("FAKE_JJ_LOG_FILE", tree.workspace.join("jj-log.txt"));
        std::env::set_var(
            "FAKE_JJ_DIFF_STATS_FILE",
            tree.workspace.join("jj-diff-stats.txt"),
        );
        std::env::set_var("FAKE_JJ_DIFF_FILE", tree.workspace.join("jj-diff.txt"));
        std::env::set_var("FAKE_TEA_LOG", &tree.tea_log);
        std::env::set_var(
            "FAKE_TEA_LOGIN_LIST_FILE",
            tree.workspace.join("tea-login-list.txt"),
        );
        std::env::set_var(
            "FAKE_TEA_PR_URL",
            "https://code.example.com/team/project/pulls/42",
        );
        std::env::remove_var("FAKE_TEA_FORCE_FAIL");
    }
}

async fn build_prompt_and_draft(
    config: &Config,
    repo: RepoState,
    selected_revset: RevsetSummary,
    form: PrForm,
    user_instructions: Option<&str>,
) -> (ContextBundle, teatui::generate::GeneratedDraft) {
    let bundle = context::collect(config, repo, form.clone(), selected_revset)
        .await
        .expect("context");
    let prompt = prompt::PromptBuild::new(
        &bundle,
        &form,
        user_instructions,
        prompt::DEFAULT_PROMPT_BYTE_BUDGET,
    );
    let client = LlmClient::from_config(&config.llm.backends[0]).expect("llm client");
    let draft = client.generate_draft(&prompt).await.expect("draft");
    (bundle, draft)
}

#[tokio::test]
async fn fake_happy_path_captures_prompt_and_pr_url() {
    let _guard = test_lock().await;
    let tree = FakeCommandTree::new().expect("fixtures");
    set_fake_env(&tree);
    tree.write_jj_outputs(
        &tree.workspace.display().to_string(),
        "Working copy is clean",
        "abc123|def456|feature/example|Keep the current change",
        "file.rs | 3 ++-",
        "diff --git a/file.rs b/file.rs\n+added line",
    )
    .expect("jj outputs");
    tree.write_tea_outputs(
        "Name URL User Default\ngitea https://code.example.com alice true",
        "creating PR\nview it at https://code.example.com/team/project/pulls/42\nthanks",
    )
    .expect("tea outputs");

    let server = FakeOllamaServer::start(
        serde_json::json!({
            "response": serde_json::to_string(&serde_json::json!({
                "branch_name": "feature/example",
                "title": "Improve PR generation",
                "body": "Summary Testing Risks",
                "review_notes": ["check stale context"]
            }))
            .expect("draft json"),
            "done": true
        })
        .to_string(),
    )
    .await
    .expect("fake ollama");

    let config = tree.config(server.base_url());
    let repo = tree.repo_state(server.base_url());
    let selected_revset = sample_revset(vec!["feature/example".into()]);
    let form = sample_form();
    let (bundle, draft) = build_prompt_and_draft(
        &config,
        repo.clone(),
        selected_revset.clone(),
        form.clone(),
        Some("Prefer concise output."),
    )
    .await;

    let mut state = teatui::generate::GenerateState::new(vec![selected_revset.clone()]);
    state.complete_context_collection(bundle);
    state.complete_generation(draft.clone());
    assert_eq!(state.phase, GeneratePhase::DraftReady);
    assert_eq!(state.form.branch_name.value, "feature/example");
    assert_eq!(state.form.title.value, "Improve PR generation");

    let requests = server.requests();
    let request = requests.first().expect("request");
    assert!(request.request_line.contains("POST /api/generate"));
    assert!(
        request
            .headers
            .iter()
            .any(|(name, _)| name.eq_ignore_ascii_case("content-type"))
    );
    assert!(request.body.contains("Prefer concise output."));
    assert!(request.body.contains("feature/example"));
    assert!(request.body.contains("Improve PR generation"));

    let plan = ExecutionPlan::from_draft(&state.form, &repo, &selected_revset, &config);
    let (tx, mut rx) = mpsc::unbounded_channel();
    let outcome = teatui::command::run_plan_sequentially(plan.clone(), tx).await;

    let mut job_events = Vec::new();
    while let Ok(Some(event)) = timeout(Duration::from_millis(100), rx.recv()).await {
        if let BackgroundEvent::Job(job) = event {
            job_events.push(job);
        }
    }

    assert!(!job_events.is_empty());
    assert_eq!(outcome.failed_step, None, "{outcome:?}");
    let tea_capture = teatui::command::capture(plan.steps[2].command.clone())
        .await
        .expect("tea capture");
    let parsed_pr_url = teatui::tea::parse_pr_url(&tea_capture.stdout);
    assert_eq!(
        parsed_pr_url.clone(),
        Some("https://code.example.com/team/project/pulls/42".into())
    );
    assert_eq!(
        outcome.pr_url.as_deref().or(parsed_pr_url.as_deref()),
        Some("https://code.example.com/team/project/pulls/42")
    );
    assert!(
        fs::read_to_string(&tree.tea_log)
            .expect("tea log")
            .contains("pr create")
    );
}

#[tokio::test]
async fn malformed_llm_json_is_reported_with_raw_response() {
    let _guard = test_lock().await;
    let tree = FakeCommandTree::new().expect("fixtures");
    set_fake_env(&tree);
    tree.write_jj_outputs(
        &tree.workspace.display().to_string(),
        "Working copy is clean",
        "abc123|def456|feature/example|Keep the current change",
        "file.rs | 3 ++-",
        "diff --git a/file.rs b/file.rs\n+added line",
    )
    .expect("jj outputs");

    let server = FakeOllamaServer::start(
        serde_json::json!({
            "response": "{\"branch_name\":\"feature/example\",\"title\":\"ok\",\"body\":\"broken\",\"review_notes\":[],\"extra\":true}",
            "done": true
        })
        .to_string(),
    )
    .await
    .expect("fake ollama");

    let config = tree.config(server.base_url());
    let repo = tree.repo_state(server.base_url());
    let selected_revset = sample_revset(vec![]);
    let form = sample_form();
    let bundle = context::collect(&config, repo, form.clone(), selected_revset)
        .await
        .expect("context");
    let client = LlmClient::from_config(&config.llm.backends[0]).expect("llm client");
    let prompt = prompt::PromptBuild::new(&bundle, &form, None, prompt::DEFAULT_PROMPT_BYTE_BUDGET);
    let err = client
        .generate_draft(&prompt)
        .await
        .expect_err("draft error");

    assert!(err.message.contains("parse generated draft JSON"));
    assert!(err.raw_response.is_some());
}

#[tokio::test]
async fn stale_context_blocks_confirmation_and_preserves_retry_state() {
    let _guard = test_lock().await;
    let tree = FakeCommandTree::new().expect("fixtures");
    set_fake_env(&tree);
    tree.write_jj_outputs(
        &tree.workspace.display().to_string(),
        "Working copy is clean",
        "zzz999|yyy888||Different change",
        "file.rs | 1 +",
        "diff --git a/file.rs b/file.rs\n+changed line",
    )
    .expect("jj outputs");

    let config = tree.config("http://127.0.0.1:11434");
    let (tx, mut rx) = mpsc::unbounded_channel();
    jj::spawn_stale_context_check(
        &config,
        tree.workspace.clone(),
        "@".into(),
        vec!["abc123".into()],
        tx,
    );

    let event = timeout(Duration::from_secs(2), rx.recv())
        .await
        .expect("stale check event")
        .expect("event");
    let reason = match event {
        BackgroundEvent::StaleCheck(StaleCheckResult::Stale { reason }) => reason,
        _ => panic!("unexpected event"),
    };

    let mut state = teatui::generate::GenerateState::new(vec![sample_revset(vec![])]);
    state.begin_confirmation_check();
    state.fail_confirmation(reason.clone());
    state.execution_plan = Some(ExecutionPlan::default());

    assert_eq!(state.phase, GeneratePhase::Failed);
    assert!(reason.contains("refresh"));
    assert!(state.execution_plan.is_some());
}

#[tokio::test]
async fn tea_pr_create_failure_retains_retryable_plan() {
    let _guard = test_lock().await;
    let tree = FakeCommandTree::new().expect("fixtures");
    set_fake_env(&tree);
    unsafe {
        std::env::set_var("FAKE_TEA_FORCE_FAIL", "1");
    }
    tree.write_jj_outputs(
        &tree.workspace.display().to_string(),
        "Working copy is clean",
        "abc123|def456|feature/example|Keep the current change",
        "file.rs | 3 ++-",
        "diff --git a/file.rs b/file.rs\n+added line",
    )
    .expect("jj outputs");
    tree.write_tea_outputs(
        "Name URL User Default\ngitea https://code.example.com alice true",
        "boom",
    )
    .expect("tea outputs");

    let server = FakeOllamaServer::start(
        serde_json::json!({
            "response": serde_json::to_string(&serde_json::json!({
                "branch_name": "feature/example",
                "title": "Improve PR generation",
                "body": "Summary Testing Risks",
                "review_notes": []
            }))
            .expect("draft json"),
            "done": true
        })
        .to_string(),
    )
    .await
    .expect("fake ollama");

    let config = tree.config(server.base_url());
    let repo = tree.repo_state(server.base_url());
    let selected_revset = sample_revset(vec!["feature/example".into()]);
    let form = sample_form();
    let (bundle, draft) = build_prompt_and_draft(
        &config,
        repo.clone(),
        selected_revset.clone(),
        form.clone(),
        None,
    )
    .await;
    let mut state = teatui::generate::GenerateState::new(vec![selected_revset.clone()]);
    state.complete_context_collection(bundle);
    state.complete_generation(draft);
    let plan = ExecutionPlan::from_draft(&state.form, &repo, &selected_revset, &config);

    let (tx, _rx) = mpsc::unbounded_channel();
    let outcome = teatui::command::run_plan_sequentially(plan.clone(), tx).await;
    state.begin_execution();
    state.execution_plan = Some(plan.clone());
    state.fail_execution(Some(2), outcome.message.clone().expect("message"));

    assert_eq!(outcome.failed_step, Some(2));
    assert!(outcome.pr_url.is_none());
    assert_eq!(state.phase, GeneratePhase::Failed);
    assert_eq!(state.execution_plan.as_ref(), Some(&plan));
}
