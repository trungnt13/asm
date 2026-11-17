//! Enterprise authentication and revocation preserve ordinary tools and accumulated history.

#[derive(Clone, Copy)]
enum EnterpriseLifecycle {
    Policy,
    Removal,
    RefreshPermissions,
}
