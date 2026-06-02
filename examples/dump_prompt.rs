// Throwaway: assemble the Generate prompt for a base..head range exactly as the
// app does, and print it (plus a size breakdown). Run with:
//   cargo run --example dump_prompt -- <base> <head>
use teatui::domain::{ContextJob, ContextResult, PromptForm, build_prompt};
use teatui::runtime::{Job, JobOutcome};

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let base = args.get(1).cloned().unwrap_or_else(|| "trunk()".into());
    let head = args.get(2).cloned().unwrap_or_else(|| "@".into());

    let job = Box::new(ContextJob {
        jj_binary: "jj".into(),
        base: base.clone(),
        head: head.clone(),
        diff_byte_budget: 64 * 1024,
    });
    let JobOutcome::Done(out) = job.run() else {
        eprintln!("job did not finish");
        return;
    };
    let result = *out.downcast::<ContextResult>().expect("ContextResult");
    let bundle = match result {
        ContextResult::Ready(b) => b,
        ContextResult::Errored { message } => {
            eprintln!("context error: {message}");
            return;
        }
    };

    eprintln!("changes: {}", bundle.changes.len());
    for (i, c) in bundle.changes.iter().enumerate() {
        eprintln!(
            "  [{i}] {:<50} stat={} bytes body={} bytes",
            c.subject,
            c.diff_stat.len(),
            c.body.len()
        );
    }
    eprintln!(
        "aggregate diff={} bytes truncated={}",
        bundle.aggregate.diff.len(),
        bundle.aggregate.diff_truncated
    );

    let form = PromptForm {
        head,
        base,
        branch: String::new(),
        title: String::new(),
        description: String::new(),
    };
    let built = build_prompt(&bundle, &form);
    eprintln!("--- section bytes ---");
    for s in &built.manifest.sections {
        eprintln!("  {:<22} {} bytes", s.name, s.bytes);
    }
    eprintln!("total prompt: {} bytes", built.manifest.total_bytes);
    eprintln!("--- prompt ---");
    println!("{}", built.prompt);
}
