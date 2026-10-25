impl super::ControllerLoop<'_> {
    pub(super) async fn supervise_local_workers(&mut self) {
        self.local_worker_health
            .supervise(
                &self.journal,
                &self.docker,
                &self.selected_engine,
                self.current.connections(),
                &self.pending_retirements,
            )
            .await;
    }
}
