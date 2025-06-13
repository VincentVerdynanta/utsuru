use std::pin::Pin;
use webrtc::media::Sample;

use crate::error::Error;

mod discord;

pub use discord::DiscordLiveBuilder;

pub trait Mirror {
    fn write_audio_sample<'a>(
        &'a self,
        payload: &'a Sample,
    ) -> Pin<Box<dyn Future<Output = Result<(), Error>> + Send + 'a>>;

    fn write_video_sample<'a>(
        &'a self,
        payload: &'a Sample,
    ) -> Pin<Box<dyn Future<Output = Result<(), Error>> + Send + 'a>>;

    fn call_connected_callback(&self) -> Result<(), Error> {
        Ok(())
    }

    fn close(&self);
}
