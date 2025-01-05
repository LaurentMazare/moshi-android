#[derive(Debug, PartialEq, Clone)]
pub struct BuildInfo {
    pub build_timestamp: &'static str,
    pub build_date: &'static str,
    pub git_branch: &'static str,
    pub git_timestamp: &'static str,
    pub git_date: &'static str,
    pub git_hash: &'static str,
    pub git_describe: &'static str,
    pub rustc_host_triple: &'static str,
    pub rustc_version: &'static str,
    pub cargo_target_triple: &'static str,
}

pub const BUILD_INFO: BuildInfo = BuildInfo {
    build_timestamp: env!("VERGEN_BUILD_TIMESTAMP"),
    build_date: env!("VERGEN_BUILD_DATE"),
    git_branch: env!("VERGEN_GIT_BRANCH"),
    git_timestamp: env!("VERGEN_GIT_COMMIT_TIMESTAMP"),
    git_date: env!("VERGEN_GIT_COMMIT_DATE"),
    git_hash: env!("VERGEN_GIT_SHA"),
    git_describe: env!("VERGEN_GIT_DESCRIBE"),
    rustc_host_triple: env!("VERGEN_RUSTC_HOST_TRIPLE"),
    rustc_version: env!("VERGEN_RUSTC_SEMVER"),
    cargo_target_triple: env!("VERGEN_CARGO_TARGET_TRIPLE"),
};
