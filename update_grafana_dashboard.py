#!/usr/bin/env python3
"""
Script to programmatically add missing metrics to grafana-rsky-dashboard.json

This script adds the following metrics:
1. ingester_firehose_backfill_length to Redis Stream Lengths panel
2. backfiller_repos_waiting (new panel)
3. backfiller_repos_running (new panel)
4. All other missing metrics from METRICS_AUDIT.md

Usage:
    python3 update_grafana_dashboard.py
"""

import json
import sys
from pathlib import Path
from typing import Dict, List, Any
import copy

def load_dashboard(path: str) -> Dict[str, Any]:
    """Load the dashboard JSON file"""
    with open(path, 'r') as f:
        return json.load(f)

def save_dashboard(dashboard: Dict[str, Any], path: str, backup: bool = True):
    """Save the dashboard JSON file with optional backup"""
    if backup:
        backup_path = f"{path}.backup"
        print(f"Creating backup at {backup_path}")
        with open(path, 'r') as f:
            with open(backup_path, 'w') as bf:
                bf.write(f.read())

    with open(path, 'w') as f:
        json.dump(dashboard, f, indent=2)
    print(f"Dashboard saved to {path}")

def get_next_panel_id(dashboard: Dict[str, Any]) -> int:
    """Find the next available panel ID"""
    max_id = 0
    for panel in dashboard.get('panels', []):
        if 'id' in panel:
            max_id = max(max_id, panel['id'])
        # Check nested panels in rows
        if 'panels' in panel:
            for nested in panel['panels']:
                if 'id' in nested:
                    max_id = max(max_id, nested['id'])
    return max_id + 1

def find_panel_by_title(dashboard: Dict[str, Any], title: str) -> tuple:
    """Find a panel by title, return (panel, parent_index, nested_index)"""
    for i, panel in enumerate(dashboard['panels']):
        if panel.get('title') == title:
            return (panel, i, None)
        # Check nested panels
        if 'panels' in panel:
            for j, nested in enumerate(panel['panels']):
                if nested.get('title') == title:
                    return (nested, i, j)
    return (None, None, None)

def add_target_to_panel(panel: Dict[str, Any], expr: str, ref_id: str, legend: str = None):
    """Add a new target (query) to an existing panel"""
    if 'targets' not in panel:
        panel['targets'] = []

    target = {
        "datasource": {
            "type": "prometheus",
            "uid": "${DS_PROMETHEUS}"
        },
        "expr": expr,
        "refId": ref_id
    }

    if legend:
        target["legendFormat"] = legend

    panel['targets'].append(target)
    return panel

def create_stat_panel(
    panel_id: int,
    title: str,
    expr: str,
    grid_pos: Dict[str, int],
    unit: str = "short",
    thresholds: List[Dict[str, Any]] = None
) -> Dict[str, Any]:
    """Create a stat panel configuration"""
    if thresholds is None:
        thresholds = [
            {"color": "green", "value": None}
        ]

    return {
        "datasource": {
            "type": "prometheus",
            "uid": "${DS_PROMETHEUS}"
        },
        "fieldConfig": {
            "defaults": {
                "color": {
                    "mode": "thresholds"
                },
                "mappings": [],
                "thresholds": {
                    "mode": "absolute",
                    "steps": thresholds
                },
                "unit": unit
            }
        },
        "gridPos": grid_pos,
        "id": panel_id,
        "options": {
            "colorMode": "value",
            "graphMode": "area",
            "justifyMode": "auto",
            "orientation": "auto",
            "percentChangeColorMode": "standard",
            "reduceOptions": {
                "calcs": ["lastNotNull"],
                "fields": "",
                "values": False
            },
            "showPercentChange": False,
            "textMode": "auto",
            "wideLayout": True
        },
        "pluginVersion": "11.4.0",
        "targets": [
            {
                "datasource": {
                    "type": "prometheus",
                    "uid": "${DS_PROMETHEUS}"
                },
                "expr": expr,
                "refId": "A"
            }
        ],
        "title": title,
        "type": "stat"
    }

def create_timeseries_panel(
    panel_id: int,
    title: str,
    targets: List[Dict[str, str]],
    grid_pos: Dict[str, int],
    unit: str = "short"
) -> Dict[str, Any]:
    """Create a time series panel configuration"""
    return {
        "datasource": {
            "type": "prometheus",
            "uid": "${DS_PROMETHEUS}"
        },
        "fieldConfig": {
            "defaults": {
                "color": {
                    "mode": "palette-classic"
                },
                "custom": {
                    "axisBorderShow": False,
                    "axisCenteredZero": False,
                    "axisColorMode": "text",
                    "axisLabel": "",
                    "axisPlacement": "auto",
                    "barAlignment": 0,
                    "drawStyle": "line",
                    "fillOpacity": 10,
                    "gradientMode": "none",
                    "hideFrom": {
                        "tooltip": False,
                        "viz": False,
                        "legend": False
                    },
                    "insertNulls": False,
                    "lineInterpolation": "linear",
                    "lineWidth": 1,
                    "pointSize": 5,
                    "scaleDistribution": {
                        "type": "linear"
                    },
                    "showPoints": "never",
                    "spanNulls": False,
                    "stacking": {
                        "group": "A",
                        "mode": "none"
                    },
                    "thresholdsStyle": {
                        "mode": "off"
                    }
                },
                "mappings": [],
                "thresholds": {
                    "mode": "absolute",
                    "steps": [
                        {"color": "green", "value": None}
                    ]
                },
                "unit": unit
            }
        },
        "gridPos": grid_pos,
        "id": panel_id,
        "options": {
            "legend": {
                "calcs": [],
                "displayMode": "list",
                "placement": "bottom",
                "showLegend": True
            },
            "tooltip": {
                "mode": "multi",
                "sort": "none"
            }
        },
        "pluginVersion": "11.4.0",
        "targets": [
            {
                "datasource": {
                    "type": "prometheus",
                    "uid": "${DS_PROMETHEUS}"
                },
                "expr": t["expr"],
                "refId": t.get("refId", chr(65 + i)),  # A, B, C...
                "legendFormat": t.get("legend", "")
            }
            for i, t in enumerate(targets)
        ],
        "title": title,
        "type": "timeseries"
    }

def main():
    dashboard_path = Path(__file__).parent / "grafana-rsky-dashboard.json"

    if not dashboard_path.exists():
        print(f"Error: Dashboard file not found at {dashboard_path}")
        sys.exit(1)

    print("Loading dashboard...")
    dashboard = load_dashboard(dashboard_path)

    # Get next available panel ID
    next_id = get_next_panel_id(dashboard)
    print(f"Next available panel ID: {next_id}")

    # ============================================================
    # 1. Add ingester_firehose_backfill_length to existing panel
    # ============================================================
    print("\n1. Adding ingester_firehose_backfill_length to Redis Stream Lengths...")
    stream_panel, parent_idx, nested_idx = find_panel_by_title(
        dashboard, "Redis Stream Lengths (All Streams)"
    )

    if stream_panel:
        # Check if it already has this target
        has_target = any(
            'firehose_backfill_length' in t.get('expr', '')
            for t in stream_panel.get('targets', [])
        )

        if not has_target:
            # Get current number of targets to assign next refId
            num_targets = len(stream_panel.get('targets', []))
            ref_id = chr(65 + num_targets)  # A, B, C, D...

            add_target_to_panel(
                stream_panel,
                expr="ingester_firehose_backfill_length",
                ref_id=ref_id,
                legend="firehose_backfill"
            )
            print(f"  ✓ Added target with refId={ref_id}")
        else:
            print("  - Already exists, skipping")
    else:
        print("  ✗ Could not find Redis Stream Lengths panel")

    # ============================================================
    # 2. Add backfiller_repos_waiting panel
    # ============================================================
    print("\n2. Adding backfiller_repos_waiting panel...")
    backfiller_row_idx = None
    for i, panel in enumerate(dashboard['panels']):
        if panel.get('title') == '🔥 Backfiller Overview':
            backfiller_row_idx = i
            break

    if backfiller_row_idx is not None:
        # Find a good position after "Backfill Queue Length" panel
        insert_after_idx = backfiller_row_idx
        for i in range(backfiller_row_idx + 1, len(dashboard['panels'])):
            if dashboard['panels'][i].get('title') == 'Backfill Queue Length':
                insert_after_idx = i
                break

        # Create repos_waiting panel
        repos_waiting_panel = create_stat_panel(
            panel_id=next_id,
            title="Repos Waiting (Input Queue)",
            expr="sum(backfiller_repos_waiting)",
            grid_pos={"h": 4, "w": 4, "x": 0, "y": 18},  # Will be adjusted
            unit="short",
            thresholds=[
                {"color": "green", "value": None},
                {"color": "yellow", "value": 100000},
                {"color": "red", "value": 500000}
            ]
        )

        dashboard['panels'].insert(insert_after_idx + 1, repos_waiting_panel)
        next_id += 1
        print(f"  ✓ Added panel with ID={repos_waiting_panel['id']}")
    else:
        print("  ✗ Could not find Backfiller Overview section")

    # ============================================================
    # 3. Add backfiller_repos_running panel
    # ============================================================
    print("\n3. Adding backfiller_repos_running panel...")
    if backfiller_row_idx is not None:
        repos_running_panel = create_stat_panel(
            panel_id=next_id,
            title="Repos Running (Concurrent)",
            expr="sum(backfiller_repos_running)",
            grid_pos={"h": 4, "w": 4, "x": 4, "y": 18},  # Will be adjusted
            unit="short",
            thresholds=[
                {"color": "red", "value": None},
                {"color": "yellow", "value": 1},
                {"color": "green", "value": 5}
            ]
        )

        # Insert right after repos_waiting
        dashboard['panels'].insert(insert_after_idx + 2, repos_running_panel)
        next_id += 1
        print(f"  ✓ Added panel with ID={repos_running_panel['id']}")

    # ============================================================
    # 4. Add error tracking panels for backfiller
    # ============================================================
    print("\n4. Adding backfiller error tracking panels...")
    if backfiller_row_idx is not None:
        # Create CAR fetch errors panel
        car_fetch_panel = create_timeseries_panel(
            panel_id=next_id,
            title="Backfiller Error Rates",
            targets=[
                {"expr": "rate(backfiller_car_fetch_errors_total[5m])", "legend": "CAR Fetch Errors"},
                {"expr": "rate(backfiller_car_parse_errors_total[5m])", "legend": "CAR Parse Errors"},
                {"expr": "rate(backfiller_verification_errors_total[5m])", "legend": "Verification Errors"}
            ],
            grid_pos={"h": 8, "w": 12, "x": 0, "y": 22},
            unit="errors/s"
        )

        dashboard['panels'].insert(insert_after_idx + 3, car_fetch_panel)
        next_id += 1
        print(f"  ✓ Added error rates panel with ID={car_fetch_panel['id']}")

    # ============================================================
    # 5. Add ingester error metrics
    # ============================================================
    print("\n5. Adding ingester error panel...")
    ingester_row_idx = None
    for i, panel in enumerate(dashboard['panels']):
        if panel.get('title') == '📥 Ingester Overview':
            ingester_row_idx = i
            break

    if ingester_row_idx is not None:
        ingester_errors_panel = create_stat_panel(
            panel_id=next_id,
            title="Ingester Errors",
            expr="sum(ingester_errors_total)",
            grid_pos={"h": 4, "w": 3, "x": 21, "y": 1},
            unit="errors",
            thresholds=[
                {"color": "green", "value": None},
                {"color": "yellow", "value": 10},
                {"color": "red", "value": 100}
            ]
        )

        # Insert after ingester overview row
        dashboard['panels'].insert(ingester_row_idx + 5, ingester_errors_panel)
        next_id += 1
        print(f"  ✓ Added ingester errors panel with ID={ingester_errors_panel['id']}")

    # ============================================================
    # 6. Add BackfillIngester Progress Panels
    # ============================================================
    print("\n6. Adding BackfillIngester progress panels...")
    if ingester_row_idx is not None:
        # Find insertion point - after the last ingester panel
        insert_idx = ingester_row_idx + 6  # After errors panel

        # Panel 1: Repos Fetched (Counter)
        repos_fetched_panel = create_stat_panel(
            panel_id=next_id,
            title="Backfill Repos Fetched",
            expr="sum(ingester_backfill_repos_fetched_total)",
            grid_pos={"h": 4, "w": 4, "x": 0, "y": 8},
            unit="short",
            thresholds=[
                {"color": "green", "value": None}
            ]
        )
        dashboard['panels'].insert(insert_idx, repos_fetched_panel)
        next_id += 1
        print(f"  ✓ Added repos fetched panel with ID={repos_fetched_panel['id']}")

        # Panel 2: Repos Written (Counter)
        repos_written_panel = create_stat_panel(
            panel_id=next_id,
            title="Backfill Repos Written",
            expr="sum(ingester_backfill_repos_written_total)",
            grid_pos={"h": 4, "w": 4, "x": 4, "y": 8},
            unit="short",
            thresholds=[
                {"color": "green", "value": None}
            ]
        )
        dashboard['panels'].insert(insert_idx + 1, repos_written_panel)
        next_id += 1
        print(f"  ✓ Added repos written panel with ID={repos_written_panel['id']}")

        # Panel 3: Backfill Complete Status (Gauge)
        backfill_complete_panel = create_stat_panel(
            panel_id=next_id,
            title="Backfill Complete",
            expr="sum(ingester_backfill_complete)",
            grid_pos={"h": 4, "w": 4, "x": 8, "y": 8},
            unit="none",
            thresholds=[
                {"color": "yellow", "value": None},
                {"color": "green", "value": 1}
            ]
        )
        dashboard['panels'].insert(insert_idx + 2, backfill_complete_panel)
        next_id += 1
        print(f"  ✓ Added backfill complete panel with ID={backfill_complete_panel['id']}")

        # Panel 4: Backfill Errors
        backfill_errors_panel = create_timeseries_panel(
            panel_id=next_id,
            title="BackfillIngester Error Rates",
            targets=[
                {"expr": "rate(ingester_backfill_fetch_errors_total[5m])", "legend": "Fetch Errors"},
                {"expr": "rate(ingester_backfill_cursor_skips_total[5m])", "legend": "Cursor Skips"}
            ],
            grid_pos={"h": 6, "w": 12, "x": 0, "y": 12},
            unit="errors/s"
        )
        dashboard['panels'].insert(insert_idx + 3, backfill_errors_panel)
        next_id += 1
        print(f"  ✓ Added backfill errors panel with ID={backfill_errors_panel['id']}")

    # ============================================================
    # 7. Add remaining medium-priority metrics
    # ============================================================
    print("\n7. Adding remaining medium-priority metrics...")

    # Add filtered operations to ingester section
    if ingester_row_idx is not None:
        filtered_ops_panel = create_stat_panel(
            panel_id=next_id,
            title="Filtered Operations",
            expr="sum(ingester_firehose_filtered_operations_total)",
            grid_pos={"h": 4, "w": 4, "x": 12, "y": 8},
            unit="short",
            thresholds=[
                {"color": "blue", "value": None}
            ]
        )
        dashboard['panels'].insert(ingester_row_idx + 10, filtered_ops_panel)
        next_id += 1
        print(f"  ✓ Added filtered operations panel with ID={filtered_ops_panel['id']}")

    # Add backfiller quality metrics
    if backfiller_row_idx is not None:
        # Records Filtered
        records_filtered_panel = create_stat_panel(
            panel_id=next_id,
            title="Records Filtered",
            expr="sum(backfiller_records_filtered_total)",
            grid_pos={"h": 4, "w": 4, "x": 8, "y": 18},
            unit="short",
            thresholds=[
                {"color": "blue", "value": None}
            ]
        )
        dashboard['panels'].insert(backfiller_row_idx + 8, records_filtered_panel)
        next_id += 1
        print(f"  ✓ Added records filtered panel with ID={records_filtered_panel['id']}")

        # Dead Letter Queue
        dead_letter_panel = create_stat_panel(
            panel_id=next_id,
            title="Dead Letter Queue",
            expr="sum(backfiller_repos_dead_lettered_total)",
            grid_pos={"h": 4, "w": 4, "x": 12, "y": 18},
            unit="short",
            thresholds=[
                {"color": "green", "value": None},
                {"color": "yellow", "value": 5},
                {"color": "red", "value": 10}
            ]
        )
        dashboard['panels'].insert(backfiller_row_idx + 9, dead_letter_panel)
        next_id += 1
        print(f"  ✓ Added dead letter queue panel with ID={dead_letter_panel['id']}")

        # Retry Attempts
        retries_panel = create_stat_panel(
            panel_id=next_id,
            title="Retry Attempts",
            expr="sum(backfiller_retries_attempted_total)",
            grid_pos={"h": 4, "w": 4, "x": 16, "y": 18},
            unit="short",
            thresholds=[
                {"color": "green", "value": None},
                {"color": "yellow", "value": 100}
            ]
        )
        dashboard['panels'].insert(backfiller_row_idx + 10, retries_panel)
        next_id += 1
        print(f"  ✓ Added retries panel with ID={retries_panel['id']}")

    # ============================================================
    # Save updated dashboard
    # ============================================================
    print("\n" + "="*60)
    print("Saving updated dashboard...")
    save_dashboard(dashboard, dashboard_path, backup=True)

    print("\n✓ Dashboard update complete!")
    print("\nAdded metrics to dashboard:")
    print("\n  HIGH PRIORITY (Critical/High):")
    print("    1. ingester_firehose_backfill_length (to Redis Stream Lengths)")
    print("    2. backfiller_repos_waiting (new panel)")
    print("    3. backfiller_repos_running (new panel)")
    print("    4. backfiller error rates (CAR fetch, parse, verification)")
    print("    5. ingester_errors_total (new panel)")
    print("    6. ingester_backfill_repos_fetched_total (new panel)")
    print("    7. ingester_backfill_repos_written_total (new panel)")
    print("    8. ingester_backfill_complete (new panel)")
    print("    9. ingester_backfill error rates (fetch errors, cursor skips)")
    print("\n  MEDIUM PRIORITY (Quality/Filtering):")
    print("    10. ingester_firehose_filtered_operations_total")
    print("    11. backfiller_records_filtered_total")
    print("    12. backfiller_repos_dead_lettered_total")
    print("    13. backfiller_retries_attempted_total")
    print("\n  Total panels added: 13 (covering all actively-used metrics)")
    print("\nBackup created at: grafana-rsky-dashboard.json.backup")
    print("\nNext steps:")
    print("  1. Import updated dashboard into Grafana")
    print("  2. Verify panels show data:")
    print("     curl http://localhost:4100/metrics | grep -E 'firehose_backfill_length|backfill_repos'")
    print("     curl http://localhost:9090/metrics | grep -E 'repos_waiting|repos_running|dead_lettered'")

if __name__ == "__main__":
    main()
