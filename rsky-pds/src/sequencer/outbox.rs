use crate::sequencer::events::SeqEvt;

#[derive(Debug, Clone)]
pub struct OutboxOpts {
    pub max_buffer_size: usize,
}

#[derive(Debug, Clone)]
pub struct Outbox {
    caught_up: bool,
    pub last_seen: i8,
    pub cutover_buffer: Vec<SeqEvt>,
}
