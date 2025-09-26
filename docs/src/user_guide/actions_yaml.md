# Actions Configuration (actions.yaml)

The `actions.yaml` file allows you to define custom actions that can be triggered when a monitor finds a match. These actions are JavaScript files that can inspect and modify the `MonitorMatch` object before it is passed to the notifiers.

This enables dynamic enrichment of alert data, custom formatting, or even conditional logic that can alter the notification content based on the match details.

> **Note**: Actions require the `js_executor` binary to be available. When building from source, make sure to build both the main application and the js_executor: `cargo build --release --manifest-path crates/js_executor/Cargo.toml`. Docker deployments include this automatically.

## Basic Structure

An `actions.yaml` file contains a list of action definitions. Each action has a `name` and a `file` path pointing to the script.

```yaml
actions:
  - name: "my-js-action"
    file: "path/to/your/script.js"
```

## Action Fields

*   **`name`** (string, required): A unique, human-readable name for the action. This name is referenced in the `on_match` list of a monitor in [`monitors.yaml`](./monitors_yaml.md).
*   **`file`** (string, required): The path to the JavaScript file to be executed. The path should be relative to the project root or an absolute path.

## JavaScript Action Environment

When an action script is executed, it has access to a global `match` object. This object is a mutable representation of the `MonitorMatch` data.

The script can modify the fields of this `match` object. After the script finishes, the modified `match` object is what gets sent to the notifiers.

### The `match` Object

The `match` object available in the script has the following structure (fields are the same as those available in [Notifier Templates](./notifier_templating.md)):

*   `monitor_id` (number)
*   `monitor_name` (string)
*   `notifier_name` (string)
*   `block_number` (number)
*   `transaction_hash` (string)
*   `match_data` (object): Contains either a `Transaction` or `Log` object with detailed data.

### Example Script

Here is an example of a simple `action.js` that modifies the `monitor_name`:

```javascript
// script.js

// The global 'match' object is available.
console.log("Action triggered for monitor:", match.monitor_name);

// Modify the monitor name.
match.monitor_name = "Enriched: " + match.monitor_name;

console.log("Modified monitor name:", match.monitor_name);
```

## How to Use Actions

To use an action, you need to:

1.  Define the action in `actions.yaml`.
2.  Reference the action's `name` in the `on_match` field of a monitor in your `monitors.yaml` file.

See the documentation for [`monitors.yaml`](./monitors_yaml.md) for more details on the `on_match` field.

For a complete, working example, see [Example 10: On-Match Action Script](https://github.com/isSerge/argus-rs/tree/main/examples/10_on_match_script/README.md).
