mod app;
pub mod clients;
pub mod api;
pub mod dates;
pub mod times;
pub use app::AppState;
pub use clients::AgilePredictClient;
pub use clients::FoxESSClient;
pub use clients::OctopusClient;

