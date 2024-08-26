#[derive(Clone, Debug)]
pub struct ZMessage {
    pub id_tx: u32,
    pub nout: u32,
    pub sender: Option<String>,
    pub recipient: String,
    pub subject: String,
    pub body: String,
    pub timestamp: u32,
    pub height: u32,
    pub incoming: bool,
}
