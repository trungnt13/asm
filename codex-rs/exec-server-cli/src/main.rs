//! Standalone executor entry point; startup and sandbox dispatch are shared with Codex.

#[cfg(all(
    target_os = "linux",
    target_env = "musl",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
#[global_allocator]
static ALLOCATOR: tikv_jemallocator::Jemalloc = tikv_jemallocator::Jemalloc;

fn main() -> anyhow::Result<()> {
    #[cfg(target_os = "linux")]
    codex_arg0::reexec_with_process_name("asm-exec-server");
    codex_build_info::initialize!();
    codex_exec_server_cli::run_standalone()
}
