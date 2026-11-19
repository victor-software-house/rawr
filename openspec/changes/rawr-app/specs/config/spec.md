# Spec Delta

## MODIFIED Requirements

### Requirement: Config keys with defaults

rawr SHALL read `$XDG_CONFIG_HOME/rawr/config.yaml`, or `~/.config/rawr/config.yaml`, with `history.per_pane` (default 5), `restart.wait` (default `5m`), `drain.interrupt` (default `2m`) and `drain.grace` (default `1m`), and SHALL use the defaults when the file is absent.

#### Scenario: A bad value

- **WHEN** `config.yaml` line 2 reads `per_pane: -1`
- **THEN** rawr exits non-zero and the error names `config.yaml` and line 2

#### Scenario: The picker's key

- **WHEN** `config.yaml` sets `picker.rows`
- **THEN** rawr exits non-zero and the error names the unknown key `picker`
