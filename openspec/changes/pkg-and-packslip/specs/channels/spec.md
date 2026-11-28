## ADDED Requirements

### Requirement: The pkg offers the setup pieces

The pkg's Installer SHALL offer "Shell line" and "Rex module" as selected choices under Customize, and the first rawr daemon of each user SHALL apply the chosen pieces without asking.

#### Scenario: Rex module left out

- **WHEN** the user clears "Rex module" in Installer and opens rawr afterwards
- **THEN** the startup file holds rawr's block, `init.lua` holds none, and no checklist appears

### Requirement: The pkg keeps itself current

The updater SHALL install a newer release only when its pkg is notarized and signed by the updater's team.

#### Scenario: A newer release

- **WHEN** release v0.0.6 is published and the installed rawr is v0.0.5
- **THEN** within 6 hours `rawr --version` prints `rawr 0.0.6`
