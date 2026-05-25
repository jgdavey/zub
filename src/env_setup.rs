use crate::identity::Identity;
use std::env;

pub const CONFIG: &'static str = "ZUB_CONFIG";
pub const INSTANCE: &'static str = "ZUB_INSTANCE";
pub const ROOT:  &'static str = "ZUB_ROOT";

pub fn apply(id: &Identity) {
    env::set_var(CONFIG, &id.config_path);
    env::set_var(ROOT, &id.root);
    env::set_var(INSTANCE, &id.name);
}
