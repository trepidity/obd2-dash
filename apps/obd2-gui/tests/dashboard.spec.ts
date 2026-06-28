import { expect, test } from "@playwright/test";
import { writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

test("dashboard renders and raw tab opens", async ({ page }) => {
  const consoleIssues: string[] = [];
  page.on("console", (message) => {
    if (message.type() === "error" || message.type() === "warning") {
      consoleIssues.push(`${message.type()}: ${message.text()}`);
    }
  });

  await page.goto("http://127.0.0.1:5173/");
  await expect(page).toHaveTitle("OBD2 Dash");
  await expect(page.getByText("Enhanced PIDs")).toBeVisible();
  await expect(page.getByText("Alerts")).toBeVisible();
  await expect(page.getByText("4260.0 psi")).toBeVisible();

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
      "0.000 N command=0100",
      "0.010 W 0100\\r",
      "0.050 R 4100983B0015\\r\\r>",
      "",
    ].join("\n"),
  );
  await page.getByLabel("Open recording file").setInputFiles(rawFixture);
  await expect(page.getByText("obd2-gui-").first()).toBeVisible();
  await expect(page.getByText("obd2-raw v1")).toBeVisible();

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
  await expect(page.getByText("Enhanced PIDs")).toBeVisible();

  await page.getByRole("button", { name: "Settings" }).click();
  await expect(page.getByText("Runtime settings")).toBeVisible();
  const settingsMetric = page.getByRole("main").getByRole("button", { name: "Metric" });
  await settingsMetric.click();
  await expect(settingsMetric).toHaveAttribute("aria-pressed", "true");

  await page.getByRole("button", { name: "Raw" }).click();
  await expect(page.getByText("Raw snapshot")).toBeVisible();
  await expect(page.getByText('"fuel_rail"')).toBeVisible();
  const rawPanel = page.getByTestId("raw-snapshot-panel");
  const box = await rawPanel.boundingBox();
  const viewport = page.viewportSize();
  expect(box).not.toBeNull();
  expect(viewport).not.toBeNull();
  expect(box!.height).toBeGreaterThan(400);
  expect(box!.y + box!.height).toBeGreaterThan(viewport!.height - 48);

  expect(consoleIssues).toEqual([]);
});
