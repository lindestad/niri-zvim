use zellij_tile::prelude::*;

#[derive(Default)]
struct Plugin;

register_plugin!(Plugin);

impl ZellijPlugin for Plugin {
    fn load(&mut self, _configuration: BTreeMap<String, String>) {
        hide_self();
    }
}
