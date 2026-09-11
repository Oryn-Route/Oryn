#!/usr/bin/env python3
import json
import os
import subprocess
import glob

def main():
    print("Running benchmarks...")
    # This runs the criterion benchmarks and emits JSON to the target dir
    subprocess.run(["cargo", "bench", "-p", "orynroute-routing"], check=True)
    
    # After a successful run, Criterion outputs its estimates to target/criterion/<group>/base/estimates.json
    estimates_files = glob.glob("target/criterion/**/new/estimates.json", recursive=True)
    if not estimates_files:
        print("No criterion JSON files found. Did benchmarks complete?")
        return

    report = {"benchmarks": {}}
    for f in estimates_files:
        try:
            with open(f, "r") as json_file:
                data = json.load(json_file)
                # target/criterion/bench_name/new/estimates.json
                parts = f.replace("\\", "/").split("/")
                bench_name = parts[-3]
                
                # criterion estimates.json includes median (p50), usually under `median.point_estimate`
                # wait, criterion does not explicitly store p95 by default in estimates.json
                # it stores slope, Mean, Median, StdDev.
                # However, full distributions are available in sample.json.
                sample_file = f.replace("estimates.json", "sample.json")
                if os.path.exists(sample_file):
                    with open(sample_file, "r") as sf:
                        sample_data = json.load(sf)
                        # sample.json has a list of 'iters' and 'times'
                        times = sample_data.get("times", [])
                        iters = sample_data.get("iters", [])
                        
                        if times and iters:
                            # Time per iteration
                            time_per_iter = [t / i for t, i in zip(times, iters)]
                            time_per_iter.sort()
                            
                            p50_idx = int(len(time_per_iter) * 0.50)
                            p95_idx = int(len(time_per_iter) * 0.95)
                            
                            p50 = time_per_iter[p50_idx]
                            p95 = time_per_iter[p95_idx]
                            
                            report["benchmarks"][bench_name] = {
                                "p50_ns": round(p50, 2),
                                "p95_ns": round(p95, 2)
                            }
        except Exception as e:
            print(f"Error processing {f}: {e}")

    # Write report and export p50 / p95 latencies
    os.makedirs("docs", exist_ok=True)
    with open("docs/baseline_report.json", "w") as out:
        json.dump(report, out, indent=2)
    print("Report captured and exported to docs/baseline_report.json")
    print(json.dumps(report, indent=2))

if __name__ == "__main__":
    main()
