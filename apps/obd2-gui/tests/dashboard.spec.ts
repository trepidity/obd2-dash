import { expect, test, type Page } from "@playwright/test";
import { writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

function sessionMenuButton(page: Page) {
  return page.getByRole("button", { name: "Session menu" });
}

async function openSessionMenu(page: Page) {
  const button = sessionMenuButton(page);
  await button.click();
  await expect(button).toHaveAttribute("aria-expanded", "true");
  return page.getByRole("menu");
}

async function clickSessionMenuItem(page: Page, name: RegExp) {
  const menu = await openSessionMenu(page);
  await menu.getByRole("menuitem", { name }).click();
}

async function openRecordingViaSessionMenu(page: Page, filePath: string) {
  const menu = await openSessionMenu(page);
  const fileChooser = page.waitForEvent("filechooser");
  await menu.getByRole("menuitem", { name: /Open recording/ }).click();
  await (await fileChooser).setFiles(filePath);
}

const runnerSnapshot = {
  mode: { state: "telemetry" },
  capability_state: { persistence: "cached", verification: "ready", remaining: null },
  foreground_result: null,
  vehicle: "Mock runner",
  vin: "TESTRUNNER0000001",
  vin_source: "observed",
  protocol: "CAN 11-bit",
  connection: "runner telemetry live",
  connection_state: "live",
  telemetry_fresh: true,
  voltage: 13.8,
  rpm: 700,
  speed_mph: 0,
  poll_ms: 250,
  units: "US",
  statuses: [],
  alerts: [],
  dtcs: [],
  modules: [],
  source_confidence: [],
  signals: [],
  capability_sections: [],
  active_tests_v2: [],
};

test("stale runner data is never presented as a live session", async ({ page }) => {
  await installTauriMock(page, false, {
    ...runnerSnapshot,
    vin_source: "manual",
    connection: "runner telemetry stale (4200 ms since last vehicle response)",
    connection_state: "stale",
    telemetry_fresh: false,
    runner_sample_age_ms: 4_200,
  });

  await page.goto("http://127.0.0.1:5173/");

  await expect(sessionMenuButton(page)).toContainText("Session: Stale");
  await expect(page.getByText("VIN TESTRUNNER0000001 (manual)")).toBeVisible();
  await expect(page.getByText("Engine RPM", { exact: true }).first().locator("..")).toContainText("--");
  await expect(page.getByText("Adapter voltage", { exact: true }).first().locator("..")).toContainText("unavailable");
  await expect(page.getByText("Source", { exact: true }).first().locator("..")).toContainText("runner telemetry stale");
});

async function installTauriMock(
  page: Page,
  delayedSnapshots = false,
  snapshot: Record<string, unknown> = runnerSnapshot,
) {
  await page.addInitScript(
    ({ snapshot, delayed }) => {
      const state = {
        snapshot,
        delayed,
        calls: [] as Array<{ command: string; args: Record<string, unknown> }>,
        resolvers: [] as Array<(value: unknown) => void>,
      };
      Object.assign(window, { __obdGuiMock: state });
      Object.assign(window, {
        __TAURI_INTERNALS__: {
          invoke: async (command: string, args: Record<string, unknown> = {}) => {
            state.calls.push({ command, args });
            if (command === "diagnostic_snapshot") {
              if (state.delayed) {
                return new Promise((resolve) => state.resolvers.push(resolve));
              }
              return state.snapshot;
            }
            if (command === "run_diagnostic" || command === "rescan_vehicle" || command === "cancel_foreground") {
              return "accepted";
            }
            if (command === "set_active_view") return undefined;
            return undefined;
          },
        },
      });
    },
    { snapshot, delayed: delayedSnapshots },
  );
}

async function ipcCalls(page: Page, command: string) {
  return page.evaluate((name) => {
    const state = (window as Window & { __obdGuiMock: { calls: Array<{ command: string }> } }).__obdGuiMock;
    return state.calls.filter((call) => call.command === name).length;
  }, command);
}

test("dashboard renders category rail and utility panels", async ({ page }) => {
  const consoleIssues: string[] = [];
  page.on("console", (message) => {
    if (message.type() === "error" || message.type() === "warning") {
      consoleIssues.push(`${message.type()}: ${message.text()}`);
    }
  });

  await page.goto("http://127.0.0.1:5173/");
  await expect(page).toHaveTitle("OBD2 Dash");
  await expect(page.getByTestId("category-rail")).toBeVisible();
  await expect(page.getByRole("tablist", { name: "Diagnostic categories" })).toHaveAttribute("aria-orientation", "vertical");
  const overviewTab = page.getByRole("tab", { name: /Overview\s+0 DTC \/ 0 alerts/ });
  const powertrainTab = page.getByRole("tab", { name: /Powertrain\s+13\.9 psi \/ 39\.0 g\/s/ });
  await expect(overviewTab).toBeVisible();
  await expect(overviewTab).toHaveAttribute("aria-controls", "category-panel-overview");
  await expect(overviewTab).toHaveAttribute("tabindex", "0");
  await expect(page.getByRole("tabpanel", { name: /Overview/ })).toHaveAttribute("id", "category-panel-overview");
  await expect(page.getByRole("tabpanel", { name: /Overview/ })).toHaveAttribute("tabindex", "0");
  await expect(powertrainTab).toBeVisible();
  await expect(page.getByRole("tab", { name: /Turbo\s+88\.2% \/ 88\.2%/ })).toBeVisible();
  await expect(page.getByRole("tab", { name: /Fuel\s+4260\.0 psi \/ 5510\.0 psi/ })).toBeVisible();
  await expect(page.getByRole("tab", { name: /Discovery\s+2 candidates/ })).toBeVisible();
  await expect(page.getByRole("tab", { name: /Active Tests\s+locked/ })).toBeVisible();
  await expect(page.getByRole("tab", { name: /Other\s+170\.6 F \/ 91\.4 F/ })).toBeVisible();
  await expect(page.getByText("Alerts", { exact: true })).toBeVisible();
  const telemetryBoard = page.getByTestId("telemetry-board");
  await expect(telemetryBoard.getByText("Primary telemetry")).toBeVisible();
  await expect(telemetryBoard.getByText("Evidence lane")).toBeVisible();
  await expect(telemetryBoard).toContainText("39.0 g/s");
  await expect(telemetryBoard).toContainText("4260.0 psi");
  await expect(telemetryBoard).toContainText("5510.0 psi");

  await overviewTab.focus();
  await page.keyboard.press("ArrowDown");
  await expect(powertrainTab).toBeFocused();
  await expect(powertrainTab).toHaveAttribute("aria-selected", "true");
  await expect(powertrainTab).toHaveAttribute("tabindex", "0");
  await expect(overviewTab).toHaveAttribute("tabindex", "-1");
  await expect(page.getByRole("tabpanel", { name: /Powertrain/ })).toHaveAttribute("id", "category-panel-cap:powertrain");

  await powertrainTab.click();
  await expect(page.getByText("Intake MAP")).toBeVisible();
  await expect(page.getByText("Boost", { exact: true })).toBeVisible();
  await expect(page.getByText("MAF")).toBeVisible();
  await expect(page.getByText("Derived signals")).toBeVisible();

  await page.getByRole("tab", { name: /Discovery/ }).click();
  await expect(page.getByText("Desired MAP", { exact: true })).toBeVisible();
  await expect(page.getByText("Barometer", { exact: true })).toBeVisible();
  await expect(page.getByText("Desired MAP GM $22 1542 01 candidate", { exact: true })).toBeVisible();
  await expect(page.getByText("Barometer GM $22 1251 01 candidate", { exact: true })).toBeVisible();

  await page.getByRole("tab", { name: /Turbo/ }).click();
  await expect(page.getByText("VGT vane position")).toBeVisible();

  await page.getByRole("tab", { name: /Fuel/ }).click();
  await expect(page.getByText("Injector balance")).toBeVisible();
  await expect(page.getByText("Fuel rail", { exact: true })).toBeVisible();
  await expect(page.getByText("Actual fuel rail SAE PID 01 23")).toBeVisible();
  await expect(page.getByText("Desired fuel rail GM $22 163D 01")).toBeVisible();

  await page.getByRole("tab", { name: /Active Tests/ }).click();
  await expect(page.getByText("VGT vane control")).toBeVisible();
  await expect(page.getByText("Locked active test")).toBeVisible();
  await expect(page.getByText("Safety gates")).toBeVisible();
  await expect(page.getByText("stationary_idle_only")).toBeVisible();
  await expect(page.getByText("Verified command profile")).toBeVisible();
  await expect(page.getByRole("button", { name: "Command disabled" })).toBeDisabled();

  await page.getByRole("tab", { name: /Diagnostics/ }).click();
  await expect(page.getByText("No diagnostic codes")).toBeVisible();
  await expect(page.getByText("Module scan")).toBeVisible();
  await expect(page.getByText("Diagnostic status")).toBeVisible();

  await page.getByRole("tab", { name: /Other/ }).click();
  await expect(page.getByText("Coolant")).toBeVisible();
  await expect(page.getByText("Battery voltage")).toBeVisible();

  await expect(sessionMenuButton(page)).toContainText("Session: Live");
  await clickSessionMenuItem(page, /Start recording/);
  await expect(sessionMenuButton(page)).toContainText("Session: Recording");
  await expect(page.getByText("Record", { exact: true }).locator("..").getByText("ON", { exact: true })).toBeVisible();

  await clickSessionMenuItem(page, /Stop recording/);
  await expect(sessionMenuButton(page)).toContainText("Session: Live");
  await expect(page.getByText("Record", { exact: true }).locator("..").getByText("ready", { exact: true })).toBeVisible();
  const rawFixtureName = `obd2-gui-${Date.now()}.obd2raw`;
  const rawFixture = join(tmpdir(), rawFixtureName);
  writeFileSync(
    rawFixture,
    [
      "# obd2-raw v1",
      "# transport=serial device=/dev/tty.test",
      "# started=2026-06-27T04:04:59.906Z",
      "# gm evidence=desired-map/barometer/fuel-rail/class2-dtc",
      "0.000 N command=0100",
      "0.010 W 0100\\r",
      "0.050 R 4100983B0015\\r\\r>",
      "0.100 N command=22154201",
      "0.110 W 22154201\\r",
      "0.150 R 62154267\\r\\r>",
      "0.200 N command=19FFFF00",
      "0.210 W 19FFFF00\\r",
      "0.250 R 59\\r\\r>",
      "",
    ].join("\n"),
  );
  await openRecordingViaSessionMenu(page, rawFixture);
  await expect(sessionMenuButton(page)).toContainText("Session: Replay");
  await expect(page.getByText(`Replay: ${rawFixtureName}`)).toBeVisible();
  let sessionMenu = await openSessionMenu(page);
  await expect(sessionMenu.getByRole("menuitem", { name: /Play loaded/ })).toBeEnabled();
  await page.keyboard.press("Escape");

  const recFixtureName = `obd2-gui-${Date.now()}.obd2rec`;
  const recFixture = join(tmpdir(), recFixtureName);
  const header = Buffer.from(
    JSON.stringify({
      session_id: "test-session",
      start_time: "2026-06-27T04:04:59.906Z",
      vin: "1GTHK29294E391526",
      vehicle_name: "2004 GMC Sierra",
      poll_interval_ms: 250,
    }),
  );
  const headerLen = Buffer.alloc(4);
  headerLen.writeUInt32LE(header.length, 0);
  const frame = Buffer.alloc(15);
  frame[0] = 0x01;
  frame.writeUInt32LE(1234, 1);
  frame[5] = 0x0c;
  frame.writeDoubleLE(685, 6);
  frame[14] = 0;
  writeFileSync(recFixture, Buffer.concat([Buffer.from("OBD2REC\x02", "binary"), headerLen, header, frame]));
  await openRecordingViaSessionMenu(page, recFixture);
  await expect(sessionMenuButton(page)).toContainText("Session: Replay");
  await expect(page.getByText(`Replay: ${recFixtureName}`)).toBeVisible();
  sessionMenu = await openSessionMenu(page);
  await expect(sessionMenu.getByRole("menuitem", { name: /Play loaded/ })).toBeEnabled();
  await page.keyboard.press("Escape");

  await clickSessionMenuItem(page, /Play loaded/);
  await expect(sessionMenuButton(page)).toContainText("Session: Replay");
  await expect(page.getByRole("menuitem", { name: /Pause/ })).toBeVisible();
  await page.getByRole("menuitem", { name: /Pause/ }).click();
  await expect(page.getByRole("menuitem", { name: /Resume/ })).toBeVisible();
  await page.keyboard.press("Escape");
  await clickSessionMenuItem(page, /Exit replay/);
  await expect(sessionMenuButton(page)).toContainText("Session: Live");
  await expect(page.getByRole("tab", { name: /Overview/ })).toHaveAttribute("aria-selected", "true");
  await expect(page.getByText("Fuel rail", { exact: true })).toBeVisible();

  await page.getByRole("tab", { name: /Settings/ }).click();
  await expect(page.getByText("Runtime settings")).toBeVisible();
  const settingsMetric = page.getByRole("main").getByRole("button", { name: "Metric" });
  await settingsMetric.click();
  await expect(settingsMetric).toHaveAttribute("aria-pressed", "true");

  await page.getByRole("tab", { name: /Raw/ }).click();
  await expect(page.getByText("Raw snapshot")).toBeVisible();
  await expect(page.getByText('"capability_sections"')).toBeVisible();
  await expect(page.getByText('"signals"')).toBeVisible();
  await expect(page.getByText('"source_confidence"')).toBeVisible();
  await expect(page.getByText('"active_tests_v2"')).toBeVisible();
  await expect(page.getByText('"request": "6C 10 F1 22 15 42 01"')).toBeVisible();
  const rawPanel = page.getByTestId("raw-snapshot-panel");
  const rawText = (await rawPanel.textContent()) ?? "";
  for (const legacyField of [
    '"cylinders":',
    '"vgt":',
    '"fuel_rail":',
    '"temperatures":',
    '"map_psi":',
    '"desired_map_psi":',
    '"barometric_psi":',
    '"boost_psi":',
    '"maf_g_s":',
    '"active_tests":',
  ]) {
    expect(rawText).not.toContain(legacyField);
  }
  expect(rawText).toContain('"active_tests_v2":');
  const box = await rawPanel.boundingBox();
  const viewport = page.viewportSize();
  expect(box).not.toBeNull();
  expect(viewport).not.toBeNull();
  expect(box!.height).toBeGreaterThan(400);
  expect(box!.y + box!.height).toBeGreaterThan(viewport!.height - 48);

  expect(consoleIssues).toEqual([]);
});

test("generic OBD fixture renders only exposed capability sections", async ({ page }) => {
  const consoleIssues: string[] = [];
  page.on("console", (message) => {
    if (message.type() === "error" || message.type() === "warning") {
      consoleIssues.push(`${message.type()}: ${message.text()}`);
    }
  });

  await page.goto("http://127.0.0.1:5173/?fixture=generic-obd");
  await expect(page.getByText("Generic OBD-II Vehicle")).toBeVisible();
  await expect(page.getByTestId("category-rail")).toBeVisible();
  await expect(page.getByRole("tab", { name: /Powertrain/ })).toBeVisible();
  await expect(page.getByRole("tab", { name: /Emissions/ })).toBeVisible();
  await expect(page.getByRole("tab", { name: /Diagnostics/ })).toBeVisible();
  await expect(page.getByRole("tab", { name: /Turbo/ })).toHaveCount(0);
  await expect(page.getByRole("tab", { name: /^Fuel/ })).toHaveCount(0);
  await expect(page.getByRole("tab", { name: /Active Tests/ })).toHaveCount(0);

  await page.getByRole("tab", { name: /Powertrain/ }).click();
  const powertrainSection = page.getByTestId("capability-section-powertrain");
  await expect(powertrainSection.getByText("Engine RPM")).toBeVisible();
  await expect(powertrainSection.getByText("Intake MAP")).toBeVisible();
  await expect(powertrainSection.getByText("MAF")).toBeVisible();
  await expect(powertrainSection.getByText("4.7 g/s")).toBeVisible();

  await page.getByRole("tab", { name: /Emissions/ }).click();
  const emissionsSection = page.getByTestId("capability-section-emissions");
  await expect(emissionsSection.getByText("Coolant")).toBeVisible();
  await expect(emissionsSection.getByText("Intake Air")).toBeVisible();
  await expect(emissionsSection.getByText("186.2 F")).toBeVisible();

  expect(consoleIssues).toEqual([]);
});

test("gas no-turbo fixture omits diesel and turbo capabilities", async ({ page }) => {
  const consoleIssues: string[] = [];
  page.on("console", (message) => {
    if (message.type() === "error" || message.type() === "warning") {
      consoleIssues.push(`${message.type()}: ${message.text()}`);
    }
  });

  await page.goto("http://127.0.0.1:5173/?fixture=gas-no-turbo");

  await expect(page.getByText("2020 Chevrolet Malibu 1.5L")).toBeVisible();
  await expect(page.getByRole("tab", { name: /Powertrain/ })).toBeVisible();
  await expect(page.getByRole("tab", { name: /Emissions/ })).toBeVisible();
  await expect(page.getByRole("tab", { name: /Diagnostics/ })).toBeVisible();
  await expect(page.getByRole("tab", { name: /Turbo/ })).toHaveCount(0);
  await expect(page.getByRole("tab", { name: /^Fuel/ })).toHaveCount(0);
  await expect(page.getByRole("tab", { name: /Active Tests/ })).toHaveCount(0);

  await page.getByRole("tab", { name: /Emissions/ }).click();
  const emissionsSection = page.getByTestId("capability-section-emissions");
  await expect(emissionsSection.getByText("Short-term fuel trim")).toBeVisible();
  await expect(emissionsSection.getByText("O2 sensor voltage")).toBeVisible();
  await expect(page.getByText("Fuel rail", { exact: true })).toHaveCount(0);
  await expect(page.getByText("Injector balance", { exact: true })).toHaveCount(0);

  expect(consoleIssues).toEqual([]);
});

test("transmission-capable fixture exposes transmission without diesel controls", async ({ page }) => {
  const consoleIssues: string[] = [];
  page.on("console", (message) => {
    if (message.type() === "error" || message.type() === "warning") {
      consoleIssues.push(`${message.type()}: ${message.text()}`);
    }
  });

  await page.goto("http://127.0.0.1:5173/?fixture=transmission");

  await expect(page.getByText("Transmission-capable profile")).toBeVisible();
  await expect(page.getByRole("tab", { name: /Powertrain/ })).toBeVisible();
  const transmissionTab = page.getByRole("tab", { name: /Transmission/ });
  await expect(transmissionTab).toBeVisible();
  await expect(page.getByRole("tab", { name: /Turbo/ })).toHaveCount(0);
  await expect(page.getByRole("tab", { name: /^Fuel/ })).toHaveCount(0);
  await expect(page.getByRole("tab", { name: /Active Tests/ })).toHaveCount(0);

  await transmissionTab.click();
  const transmissionSection = page.getByTestId("capability-section-transmission");
  await expect(transmissionSection.getByText("Transmission fluid temp")).toBeVisible();
  await expect(transmissionSection.getByText("Commanded gear")).toBeVisible();
  await expect(transmissionSection.getByText("TCC slip")).toBeVisible();
  await expect(transmissionSection.getByText("184.6 F")).toBeVisible();

  expect(consoleIssues).toEqual([]);
});

test("profile operating ranges color injector balance values without UI signal keys", async ({ page }) => {
  const operatingRange = {
    evaluation: "absolute_magnitude",
    desired_max: 4,
    caution_max: 6,
    desired_label: "Park/Neutral range",
    caution_label: "Drive-only range",
    outside_label: "Outside service range",
    conditions: "ECT above 180 F, accessories off, steady idle for at least 30 seconds",
    source_ref: "GM 2005 LLY Fuel Injector Balance Test with Tech 2",
  } as const;
  const balanceSignal = (key: string, rowIndex: number, value: number) => ({
    key,
    label: `Injector balance cyl ${rowIndex + 1}`,
    category: "Fuel",
    module: "ecm",
    unit: "mm3",
    value,
    state: "ok",
    confidence: "LiveObserved",
    provenance: ["test profile"],
    source_fields: null,
    request: key,
    decoder_id: "test",
    evidence_policy: "normal",
    failure_policy: "retain",
    preferred_over: null,
    evidence: null,
    composition: {
      kind: "table_row",
      table_key: "test.profile.balance",
      table_label: "Injector balance",
      row_index: rowIndex,
      row_label: `${rowIndex + 1}`,
    },
    operating_range: operatingRange,
  });
  const snapshot = {
    ...runnerSnapshot,
    signals: [
      balanceSignal("test.balance.1", 0, 4),
      balanceSignal("test.balance.2", 1, -5),
      balanceSignal("test.balance.3", 2, 6.1),
    ],
    capability_sections: [{
      id: "fuel",
      category: "Fuel",
      label: "Fuel",
      signal_keys: ["test.balance.1", "test.balance.2", "test.balance.3"],
      active_test_keys: [],
      diagnostic_service_keys: [],
      visible: true,
    }],
  };

  await installTauriMock(page, false, snapshot);
  await page.goto("http://127.0.0.1:5173/");
  await page.getByRole("tab", { name: /Fuel/ }).click();

  await expect(page.locator('td[data-range-tone="ok"]')).toContainText("+4.0 mm3");
  await expect(page.locator('td[data-range-tone="warn"]')).toContainText("-5.0 mm3");
  await expect(page.locator('td[data-range-tone="crit"]')).toContainText("+6.1 mm3");
  const legend = page.getByLabel("Injector balance operating range legend");
  await expect(legend).toContainText("Park/Neutral range |value| ≤ 4");
  await expect(legend).toContainText("Drive-only range 4 < |value| ≤ 6");
  await expect(legend).toContainText("ECT above 180 F");
});

test("Tauri polling runs at 500 ms without overlapping invokes and keeps the latest view", async ({ page }) => {
  await installTauriMock(page);
  await page.goto("http://127.0.0.1:5173/");

  await page.waitForTimeout(1_600);
  const snapshots = await ipcCalls(page, "diagnostic_snapshot");
  // One immediate read plus roughly three completion-scheduled reads.  Keep
  // this tolerance for loaded CI workers without accepting the old 2.5 s
  // cadence or a fast overlapping interval.
  expect(snapshots).toBeGreaterThanOrEqual(3);
  expect(snapshots).toBeLessThanOrEqual(5);

  await page.getByRole("tab", { name: /Raw/ }).click();
  await page.getByRole("tab", { name: /Settings/ }).click();
  await expect.poll(async () => page.evaluate(() => {
    const state = (window as Window & { __obdGuiMock: { calls: Array<{ command: string; args: { view?: string } }> } }).__obdGuiMock;
    return state.calls.filter((call) => call.command === "set_active_view").at(-1)?.args.view;
  })).toBe("settings");
});

test("a delayed snapshot never overlaps and foreground controls issue one command", async ({ page }) => {
  await installTauriMock(page, true);
  await page.goto("http://127.0.0.1:5173/");

  await page.waitForTimeout(1_200);
  expect(await ipcCalls(page, "diagnostic_snapshot")).toBe(1);

  await page.evaluate(() => {
    const state = (window as Window & { __obdGuiMock: { snapshot: unknown; delayed: boolean; resolvers: Array<(value: unknown) => void> } }).__obdGuiMock;
    state.delayed = false;
    state.resolvers.shift()?.(state.snapshot);
  });
  await expect.poll(() => ipcCalls(page, "diagnostic_snapshot")).toBeGreaterThanOrEqual(2);

  await page.getByRole("tab", { name: /Diagnostics/ }).click();
  const run = page.getByRole("button", { name: "Run diagnostic" });
  // Dispatch the two pointer-equivalent activations in one task.  A real
  // second click may see the now-disabled DOM button and wait for Playwright's
  // actionability timeout instead of exercising the command coalescer.
  await run.evaluate((button: HTMLButtonElement) => {
    button.click();
    button.click();
  });
  await expect.poll(() => ipcCalls(page, "run_diagnostic")).toBe(1);
  await expect(run).toBeDisabled();

  await page.evaluate(() => {
    const state = (window as Window & { __obdGuiMock: { snapshot: { mode: unknown } } }).__obdGuiMock;
    state.snapshot.mode = { state: "diagnostic", phase: 1, phase_total: 5, step: 1, total: 9 };
  });
  await expect(page.getByRole("button", { name: "Cancel scan" })).toBeVisible();
  await page.getByRole("button", { name: "Cancel scan" }).click();
  await expect.poll(() => ipcCalls(page, "cancel_foreground")).toBe(1);
});

test("entering replay stops live snapshot polling and exiting restarts it", async ({ page }) => {
  await installTauriMock(page);
  await page.goto("http://127.0.0.1:5173/");
  await expect.poll(() => ipcCalls(page, "diagnostic_snapshot")).toBeGreaterThanOrEqual(1);

  await page.locator('input[type="file"]').setInputFiles({
    name: "runner-replay.obd2raw",
    mimeType: "text/plain",
    buffer: Buffer.from("# obd2-raw v1\n0.000 N command=0100\n0.010 R 4100\n"),
  });
  await expect(sessionMenuButton(page)).toContainText("Session: Replay");
  const beforePause = await ipcCalls(page, "diagnostic_snapshot");
  await page.waitForTimeout(700);
  expect(await ipcCalls(page, "diagnostic_snapshot")).toBe(beforePause);

  await clickSessionMenuItem(page, /Exit replay/);
  await expect.poll(() => ipcCalls(page, "diagnostic_snapshot")).toBeGreaterThan(beforePause);
});
