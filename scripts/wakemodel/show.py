import json, sys
d = json.load(open(sys.argv[1]))
print("hours", {k: round(v, 2) for k, v in d["hours"].items()}, "phrase streams", d["n"])
print("thr | triggers (clear/noisy/overBYTE) | gate (clear/noisy/overBYTE) | FA per hour: speech / look-alikes / noise")
for r in d["rows"]:
    print(f'{r["thr"]:.1f} | {r["tpr_clear"]:.2f} {r["tpr_noisy"]:.2f} {r["tpr_over_byte"]:.2f} | {r["gate_clear"]:.2f} {r["gate_noisy"]:.2f} {r["gate_over_byte"]:.2f} | {r["fa_per_h_speech"]:6.1f} {r["fa_per_h_lookalikes"]:6.1f} {r["fa_per_h_noise"]:6.1f}')
