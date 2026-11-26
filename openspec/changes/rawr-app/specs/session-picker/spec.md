# Spec Delta

## REMOVED Requirements

### Requirement: Offer only resumable sessions

**Reason**: The picker is gone; bare `rawr` replaces it.

**Migration**: The `app` capability states the same rule for the app: see its requirements "Bare rawr opens the app" and "Resume a session".

### Requirement: Resume the chosen session

**Reason**: The picker is gone; bare `rawr` replaces it.

**Migration**: The `app` capability states the same rule for the app: see its requirements "Bare rawr opens the app" and "Resume a session".

### Requirement: Full screen, restored on exit

**Reason**: The picker is gone; bare `rawr` replaces it.

**Migration**: The `app` capability states the same rule for the app: see its requirements "Bare rawr opens the app" and "Resume a session".

### Requirement: Keyboard control

**Reason**: The picker is gone; bare `rawr` replaces it.

**Migration**: The app draws and controls its own lists, as the `app` capability states.

### Requirement: Mouse control

**Reason**: The picker is gone; bare `rawr` replaces it.

**Migration**: The app draws and controls its own lists, as the `app` capability states.

### Requirement: Hover

**Reason**: The picker is gone; bare `rawr` replaces it.

**Migration**: The app draws and controls its own lists, as the `app` capability states.

### Requirement: A title, then the folder and age

**Reason**: The picker is gone; bare `rawr` replaces it.

**Migration**: The app draws and controls its own lists, as the `app` capability states.

### Requirement: Nothing is cut

**Reason**: The picker is gone; bare `rawr` replaces it.

**Migration**: The app draws and controls its own lists, as the `app` capability states.

### Requirement: Safe titles

**Reason**: The picker is gone; bare `rawr` replaces it.

**Migration**: The `app` capability keeps the rule as "Titles are drawn safe".

### Requirement: Report a waiting picker to Rex

**Reason**: The picker is gone, and the app reports its own status record.

**Migration**: None; `rawr --startup` opens the app only when the pane has a session to resume.
