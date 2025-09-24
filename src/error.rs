use std::{
    error::Error as StdError,
    fmt::{Debug, Display, Formatter, Result as FmtResult},
};

#[derive(Debug)]
pub struct Error<E = dyn StdError + Send + Sync>
where
    E: StdError + Send + Sync + ?Sized,
{
    pub(crate) kind: ErrorType,
    pub(crate) source: Option<Box<E>>,
}

impl<E> Error<E>
where
    E: StdError + Send + Sync + ?Sized,
{
    pub const fn kind(&self) -> &ErrorType {
        &self.kind
    }
}

impl<E> Display for Error<E>
where
    E: StdError + Send + Sync + ?Sized,
{
    fn fmt(&self, f: &mut Formatter<'_>) -> FmtResult {
        match self.kind {
            ErrorType::DiscordAuth => f.write_str("discord auth failed"),
            ErrorType::DiscordIPC => f.write_str("discord client crashed"),
            ErrorType::DiscordGateway => f.write_str("discord gateway closed"),
            ErrorType::DiscordEndpoint => f.write_str("discord endpoint closed"),
            ErrorType::DiscordDAVE => f.write_str("discord dave closed"),
            ErrorType::WHIPIPC => f.write_str("whip service crashed"),
            ErrorType::WHIPPeer => f.write_str("whip rtc peer closed"),
        }
    }
}

impl StdError for Error<dyn StdError + Send + Sync> {
    fn source(&self) -> Option<&(dyn StdError + 'static)> {
        self.source
            .as_ref()
            .map(|source| &**source as &(dyn StdError + 'static))
    }
}

#[derive(Debug)]
pub enum ErrorType {
    DiscordAuth,
    DiscordIPC,
    DiscordGateway,
    DiscordEndpoint,
    DiscordDAVE,
    WHIPIPC,
    WHIPPeer,
}
