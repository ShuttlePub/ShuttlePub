/// Settings the composition root resolved at startup.
pub trait AppConfig: Send + Sync + 'static {
    /// Address the HTTP server listens on.
    fn bind_address(&self) -> &str;
}

pub trait DependOnAppConfig: Send + Sync + 'static {
    type AppConfig: AppConfig;
    fn app_config(&self) -> &Self::AppConfig;
}
