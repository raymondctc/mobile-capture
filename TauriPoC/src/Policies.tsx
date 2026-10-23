import type { Preferences } from "./api";
import { NumberField } from "./ui";
export function Policies({
  value,
  change,
  disabled = false,
}: {
  value: Preferences;
  change: (value: Preferences) => void;
  disabled?: boolean;
}) {
  return (
    <div className="policy-fields">
      <fieldset disabled={disabled}>
        <legend>Log limits</legend>
        <NumberField
          label="Maximum MB"
          value={value.limits.bytes}
          min={1}
          max={200000000}
          scale={1000000}
          change={(bytes) =>
            change({ ...value, limits: { ...value.limits, bytes } })
          }
        />
        <NumberField
          label="Maximum records"
          value={value.limits.records}
          min={1}
          max={250000}
          change={(records) =>
            change({ ...value, limits: { ...value.limits, records } })
          }
        />
        <NumberField
          label="Duration in seconds"
          value={value.limits.duration_ms}
          min={1}
          max={600000}
          scale={1000}
          change={(duration_ms) =>
            change({ ...value, limits: { ...value.limits, duration_ms } })
          }
        />
      </fieldset>
      <fieldset disabled={disabled}>
        <legend>Network limits</legend>
        <NumberField
          label="Maximum flows"
          value={value.network_policy.max_flows}
          min={1}
          max={1000000}
          change={(max_flows) =>
            change({
              ...value,
              network_policy: { ...value.network_policy, max_flows },
            })
          }
        />
        <NumberField
          label="Maximum body MiB"
          value={value.network_policy.max_body_bytes}
          min={1}
          max={33554432}
          scale={1048576}
          change={(max_body_bytes) =>
            change({
              ...value,
              network_policy: { ...value.network_policy, max_body_bytes },
            })
          }
        />
        <NumberField
          label="Maximum headers KiB"
          value={value.network_policy.max_header_bytes}
          min={1}
          max={131072}
          scale={1024}
          change={(max_header_bytes) =>
            change({
              ...value,
              network_policy: { ...value.network_policy, max_header_bytes },
            })
          }
        />
        <NumberField
          label="Total body budget MiB"
          value={value.network_policy.max_total_bytes}
          min={1048576}
          max={1073741824}
          scale={1048576}
          change={(max_total_bytes) =>
            change({
              ...value,
              network_policy: { ...value.network_policy, max_total_bytes },
            })
          }
        />
        <NumberField
          label="Admission duration in minutes"
          value={value.network_policy.duration_ms}
          min={1}
          max={86400000}
          scale={60000}
          change={(duration_ms) =>
            change({
              ...value,
              network_policy: { ...value.network_policy, duration_ms },
            })
          }
        />
      </fieldset>
      <p className="hint">
        Limits bound what is collected. Reaching a log limit does not stop
        network capture. When network admission closes, forwarding continues
        until you stop and restore routing.
      </p>
    </div>
  );
}
