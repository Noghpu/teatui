use std::env;
use std::process::Stdio;
use std::time::Duration;

use color_eyre::eyre::{Result, WrapErr, bail};
use teatui::config::Config;
use teatui::context::{ContextBundle, RepoIdentity};
use teatui::generate::{FieldState, PrForm, RevsetSummary};
use teatui::ollama::OllamaClient;
use teatui::prompt;
use teatui::repo::{
    BaseBranchInfo, BaseBranchSource, OllamaStatus, RemoteInfo, RepoState, TeaAuth, ToolStatus,
};
use tokio::process::{Child, Command};
use tokio::time::timeout;

#[tokio::main]
async fn main() -> Result<()> {
    color_eyre::install()?;

    if env::var("TEATUI_SMOKE_LIVE").ok().as_deref() != Some("1") {
        bail!("set TEATUI_SMOKE_LIVE=1 to run the live smoke helper");
    }

    let mut config = Config::load(None)?;
    let smoke = SmokeSettings::from_env()?;
    config.ollama.base_url = smoke.llama_url.clone();
    if let Some(model) = smoke.model.as_ref() {
        config.ollama.model = model.clone();
    }

    let mut llama_guard = ensure_llama_server(&smoke).await?;
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(2))
        .build()
        .wrap_err("failed to build smoke HTTP client")?;
    wait_for_endpoint(&client, &smoke.llama_url, Duration::from_secs(600)).await?;

    let prompt = build_prompt();
    let ollama = OllamaClient::new(&config)?;
    let draft = match timeout(Duration::from_secs(15), ollama.generate_draft(&prompt)).await {
        Ok(Ok(draft)) => draft,
        Ok(Err(err)) => bail!(err.message),
        Err(_) => bail!("smoke LLM request timed out after 15 seconds"),
    };

    println!("LLM branch: {}", draft.branch_name);
    println!("LLM title: {}", draft.title);

    report_gitea_preflight(&smoke).await?;

    drop(llama_guard.take());
    Ok(())
}

#[derive(Debug, Clone)]
struct SmokeSettings {
    llama_server: String,
    llama_model_path: Option<String>,
    llama_url: String,
    gitea_url: Option<String>,
    gitea_user: Option<String>,
    gitea_repo: Option<String>,
    wsl_distro: Option<String>,
    model: Option<String>,
}

impl SmokeSettings {
    fn from_env() -> Result<Self> {
        Ok(Self {
            llama_server: env::var("TEATUI_SMOKE_LLAMA_SERVER")
                .unwrap_or_else(|_| "llama-server".into()),
            llama_model_path: env::var("TEATUI_SMOKE_MODEL").ok(),
            llama_url: env::var("TEATUI_SMOKE_LLAMA_URL")
                .unwrap_or_else(|_| "http://127.0.0.1:8081".into()),
            gitea_url: env::var("TEATUI_SMOKE_GITEA_URL").ok(),
            gitea_user: env::var("TEATUI_SMOKE_GITEA_USER").ok(),
            gitea_repo: env::var("TEATUI_SMOKE_GITEA_REPO").ok(),
            wsl_distro: env::var("TEATUI_SMOKE_WSL_DISTRO").ok(),
            model: env::var("TEATUI_SMOKE_MODEL").ok(),
        })
    }
}

async fn ensure_llama_server(smoke: &SmokeSettings) -> Result<Option<Child>> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(2))
        .build()
        .wrap_err("failed to build llama reachability client")?;

    if is_reachable(&client, &smoke.llama_url).await {
        println!(
            "llama.cpp server is already reachable at {}",
            smoke.llama_url
        );
        return Ok(None);
    }

    let Some(model_path) = smoke.llama_model_path.as_ref() else {
        bail!("llama.cpp server is unreachable and TEATUI_SMOKE_MODEL is not set");
    };

    let mut child = Command::new(&smoke.llama_server);
    child
        .arg("-m")
        .arg(model_path)
        .args([
            "-ngl",
            "99",
            "-c",
            "8192",
            "-fa",
            "on",
            "--reasoning",
            "off",
        ])
        .args([
            "--port",
            smoke.llama_url.split(':').next_back().unwrap_or("8081"),
        ])
        .arg("--log-disable")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .kill_on_drop(true);

    println!("starting llama.cpp server with {}", smoke.llama_server);
    let child = child.spawn().wrap_err("failed to start llama.cpp server")?;
    Ok(Some(child))
}

async fn report_gitea_preflight(smoke: &SmokeSettings) -> Result<()> {
    if let Some(url) = smoke.gitea_url.as_ref() {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(5))
            .build()
            .wrap_err("failed to build Gitea smoke client")?;
        let response = client.get(url).send().await;
        match response {
            Ok(resp) if resp.status().is_success() || resp.status().is_redirection() => {
                println!("Gitea target reachable at {url}");
            }
            Ok(resp) => {
                bail!("Gitea target {url} returned {}", resp.status());
            }
            Err(err) => {
                bail!("failed to reach Gitea target {url}: {err}");
            }
        }

        if let Some(user) = smoke.gitea_user.as_ref() {
            println!("Gitea user: {user}");
        }
        if let Some(repo) = smoke.gitea_repo.as_ref() {
            println!("Gitea repo: {repo}");
        }
        return Ok(());
    }

    if let Some(distro) = smoke.wsl_distro.as_ref() {
        let status = Command::new("wsl.exe")
            .args(["-d", distro, "--", "sh", "-lc"])
            .arg("command -v gitea >/dev/null && command -v tea >/dev/null && command -v jj >/dev/null")
            .status()
            .await
            .wrap_err("failed to invoke WSL for Gitea preflight")?;

        if status.success() {
            println!("WSL distro {distro} has gitea, tea, and jj available");
            return Ok(());
        }
    }

    println!(
        "Gitea smoke was skipped. Set TEATUI_SMOKE_GITEA_URL for a reachable disposable target or TEATUI_SMOKE_WSL_DISTRO for WSL preflight."
    );
    Ok(())
}

fn build_prompt() -> prompt::PromptBuild {
    let repo = RepoState {
        workspace_root: Some(std::env::current_dir().unwrap_or_else(|_| ".".into())),
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
        ollama_base_url: "http://127.0.0.1:8081".into(),
        ollama_model: "smoke".into(),
        ollama: OllamaStatus::Reachable,
        blockers: Vec::new(),
    };

    let form = PrForm {
        head: FieldState::new("@"),
        branch_name: FieldState::new("feature/smoke-live"),
        base: FieldState::new("main"),
        title: FieldState::new("Smoke live PR generation"),
        description: FieldState::new("Summary\n\nTesting\n\nRisks"),
        labels: FieldState::default(),
        assignees: FieldState::default(),
        milestone: FieldState::default(),
    };

    let selected_revset = RevsetSummary::new(
        "@",
        "Smoke live revset",
        vec!["feature/smoke-live".into()],
        "1 file changed",
        1,
        vec!["abc123".into()],
        vec!["def456".into()],
        vec!["abc123 def456 Smoke live revset".into()],
        Vec::new(),
    );

    let bundle = ContextBundle {
        repo_identity: RepoIdentity {
            collected_at: std::time::SystemTime::now(),
            workspace_root: repo.workspace_root.clone(),
            remote_url: repo.remote.as_ref().map(|remote| remote.raw_url.clone()),
            base_branch: repo.base_branch.name.clone(),
            selected_revset: selected_revset.label().to_string(),
        },
        remote: repo.remote,
        form: form.clone(),
        selected_revset,
        selected_descriptions: vec!["Smoke live revset".into()],
        status: teatui::context::CommandCapture {
            command: "jj status".into(),
            stdout: "ok".into(),
            stderr: String::new(),
        },
        revset_log: teatui::context::CommandCapture {
            command: "jj log".into(),
            stdout: "ok".into(),
            stderr: String::new(),
        },
        diff_stats: teatui::context::CommandCapture {
            command: "jj diff --stat".into(),
            stdout: "ok".into(),
            stderr: String::new(),
        },
        diff: teatui::context::CommandCapture {
            command: "jj diff".into(),
            stdout: "ok".into(),
            stderr: String::new(),
        },
    };

    prompt::PromptBuild::new(
        &bundle,
        &form,
        Some("Smoke the live model and PR flow."),
        prompt::DEFAULT_PROMPT_BYTE_BUDGET,
    )
}

async fn is_reachable(client: &reqwest::Client, url: &str) -> bool {
    client.get(url).send().await.is_ok()
}

async fn wait_for_endpoint(client: &reqwest::Client, url: &str, deadline: Duration) -> Result<()> {
    let started = std::time::Instant::now();
    loop {
        if is_reachable(client, url).await {
            println!("llama.cpp server reachable at {url}");
            return Ok(());
        }

        if started.elapsed() >= deadline {
            bail!(
                "llama.cpp server at {url} did not become reachable within {:?}",
                deadline
            );
        }

        tokio::time::sleep(Duration::from_secs(1)).await;
    }
}
