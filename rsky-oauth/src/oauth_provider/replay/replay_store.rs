pub trait ReplayStore: Send + Sync {
    /**
     * Returns true if the nonce is unique within the given time frame. While not
     * strictly necessary for security purposes, the namespace should be used to
     * mitigate denial of service attacks from one client to the other.
     *
     * @param timeFrame expressed in milliseconds.
     */
    fn unique(&mut self, namespace: &str, nonce: &str, timeframe: f64) -> bool;
}
