use omnifs_sdk::Cx;
use omnifs_sdk::http::Request;

use crate::State;

pub(crate) trait DnsHttpExt {
    fn dns_message_get(&self, url: impl Into<String>) -> Request<'_, State>;
}

impl DnsHttpExt for Cx<State> {
    fn dns_message_get(&self, url: impl Into<String>) -> Request<'_, State> {
        self.http()
            .get(url)
            .header("Accept", "application/dns-message")
    }
}
