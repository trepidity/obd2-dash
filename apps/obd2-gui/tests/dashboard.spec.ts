import { expect, test } from "@playwright/test";
import { writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

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
  await expect(page.getByTestId("capability-section-fuel").getByText("4260.0 psi", { exact: true })).toBeVisible();
  await expect(page.getByTestId("capability-section-fuel").getByText("5510.0 psi", { exact: true })).toBeVisible();
  await expect(page.getByTestId("capability-section-powertrain").getByText("39.0 g/s", { exact: true })).toBeVisible();

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

  await page.getByRole("button", { name: "Record" }).click();
  await expect(page.getByRole("button", { name: "Stop Rec" })).toHaveAttribute("aria-pressed", "true");

  await page.getByRole("button", { name: "Replay" }).click();
  await expect(page.getByText("Replay controls")).toBeVisible();
  const rawFixture = join(tmpdir(), `obd2-gui-${Date.now()}.obd2raw`);
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
  await page.getByLabel("Open recording file").setInputFiles(rawFixture);
  await expect(page.getByText("obd2-gui-").first()).toBeVisible();
  await expect(page.getByText("obd2-raw v1")).toBeVisible();
  await expect(page.getByText("Evidence metadata")).toBeVisible();
  const evidenceBlock = page.getByText("Evidence metadata").locator("..");
  await expect(evidenceBlock.getByText("22154201")).toBeVisible();

  const recFixture = join(tmpdir(), `obd2-gui-${Date.now()}.obd2rec`);
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
  await page.getByLabel("Open recording file").setInputFiles(recFixture);
  await expect(page.getByText("test-session").first()).toBeVisible();
  await expect(page.getByText("OBD2REC v2")).toBeVisible();

  await page.getByRole("button", { name: "Play loaded" }).click();
  await page.getByRole("button", { name: "Pause" }).click();
  await expect(page.getByRole("button", { name: "Resume" })).toBeVisible();
  await page.getByRole("button", { name: "Exit replay" }).click();
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
