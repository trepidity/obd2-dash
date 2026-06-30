import { expect, test } from "@playwright/test";
import { writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

test("dashboard renders category tabs and utility panels", async ({ page }) => {
  const consoleIssues: string[] = [];
  page.on("console", (message) => {
    if (message.type() === "error" || message.type() === "warning") {
      consoleIssues.push(`${message.type()}: ${message.text()}`);
    }
  });

  await page.goto("http://127.0.0.1:5173/");
  await expect(page).toHaveTitle("OBD2 Dash");
  await expect(page.getByRole("tab", { name: /Overview\s+0 DTC \/ 0 alerts/ })).toBeVisible();
  await expect(page.getByRole("tab", { name: /Air \/ Boost\s+0\.0 psi \/ 39\.0 g\/s/ })).toBeVisible();
  await expect(page.getByRole("tab", { name: /Fuel \/ VGT\s+rail delta -1250\.0 psi/ })).toBeVisible();
  await expect(page.getByRole("tab", { name: /Active Tests\s+locked/ })).toBeVisible();
  await expect(page.getByRole("tab", { name: /Thermal \/ System\s+13\.8 V \/ 170\.6 F/ })).toBeVisible();
  await expect(page.getByText("Alerts", { exact: true })).toBeVisible();
  await expect(page.getByText("Fuel rail", { exact: true })).toBeVisible();
  await expect(page.getByText("4260.0 psi")).toBeVisible();
  await expect(page.getByText("5510.0 psi")).toBeVisible();
  await expect(page.getByText("Desired MAP", { exact: true })).toBeVisible();
  await expect(page.getByText("Barometer", { exact: true })).toBeVisible();
  await expect(page.getByText("39.0 g/s", { exact: true })).toBeVisible();

  await page.getByRole("tab", { name: /Air \/ Boost/ }).click();
  await expect(page.getByText("Intake MAP")).toBeVisible();
  await expect(page.getByText("Boost", { exact: true })).toBeVisible();
  await expect(page.getByText("MAF")).toBeVisible();
  await expect(page.getByText("GM $22 1542 01 candidate", { exact: true })).toBeVisible();
  await expect(page.getByText("GM $22 1251 01 candidate", { exact: true })).toBeVisible();

  await page.getByRole("tab", { name: /Fuel \/ VGT/ }).click();
  await expect(page.getByText("Enhanced PIDs")).toBeVisible();
  await expect(page.getByText("Injector balance")).toBeVisible();
  await expect(page.getByText("Fuel rail", { exact: true })).toBeVisible();
  await expect(page.getByText("Actual fuel rail GM $22 163E 01")).toBeVisible();
  await expect(page.getByText("Desired fuel rail GM $22 163D 01")).toBeVisible();

  await page.getByRole("tab", { name: /Active Tests/ }).click();
  await expect(page.getByText("VGT active test")).toBeVisible();
  await expect(page.getByText("Safety gates")).toBeVisible();
  await expect(page.getByLabel("Manual VGT vane percent")).toHaveValue("35.0");
  await expect(page.getByText("Locked: missing verified GM Class 2 actuator-control profile")).toBeVisible();
  await expect(page.getByText("Verified command profile")).toBeVisible();
  await expect(page.getByRole("button", { name: "Record blocked request" })).toBeEnabled();

  await page.getByRole("tab", { name: /Diagnostics/ }).click();
  await expect(page.getByText("No diagnostic codes")).toBeVisible();
  await expect(page.getByText("Module scan")).toBeVisible();
  await expect(page.getByText("Readiness")).toBeVisible();

  await page.getByRole("tab", { name: /Thermal \/ System/ }).click();
  await expect(page.getByText("Temperatures")).toBeVisible();
  await expect(page.getByText("Protocol")).toBeVisible();
  await expect(page.getByRole("main").getByText("J1850 VPW")).toBeVisible();

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
  await expect(page.getByText('"fuel_rail"')).toBeVisible();
  await expect(page.getByText('"source_confidence"')).toBeVisible();
  await expect(page.getByText('"active_tests"')).toBeVisible();
  await expect(page.getByText('"request": "6C 10 F1 22 15 42 01"')).toBeVisible();
  const rawPanel = page.getByTestId("raw-snapshot-panel");
  const box = await rawPanel.boundingBox();
  const viewport = page.viewportSize();
  expect(box).not.toBeNull();
  expect(viewport).not.toBeNull();
  expect(box!.height).toBeGreaterThan(400);
  expect(box!.y + box!.height).toBeGreaterThan(viewport!.height - 48);

  expect(consoleIssues).toEqual([]);
});
