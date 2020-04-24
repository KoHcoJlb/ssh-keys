fn main() {
    if cfg!(windows) {
        embed_resource::compile("./src/platform/win/ssh-agent.rc")
    }
}
