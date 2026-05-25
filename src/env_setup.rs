use crate::identity::Identity;
use std::env;

pub const CONFIG: &str = "ZUB_CONFIG";
pub const INSTANCE: &str = "ZUB_INSTANCE";
pub const ROOT: &str = "ZUB_ROOT";

pub fn apply(id: &Identity) {
    env::set_var(CONFIG, &id.config_path);
    env::set_var(ROOT, &id.root);
    env::set_var(INSTANCE, &id.name);
}
