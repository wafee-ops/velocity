import { spawn } from "node:child_process";
import * as fs from "node:fs";
import * as path from "node:path";

/**
 * Universal Engine Benchmark Tool (Velocity edition)
 *
 * Benchmarks terminal command-wrapping engines.
 * Default strategy uses Velocity's Rust runner when present.
 */

const CONFIG = {
  MARKER: process.env.ENGINE_MARKER || "__VELOCITY_EXIT__",
  SANDBOX:
    process.env.ENGINE_SANDBOX || path.resolve(process.cwd(), "temp_sandbox"),
  REPORT_PATH: path.join(process.cwd(), "docs", "engine-benchmarks.md"),
  TOTAL_COMMANDS: Number.parseInt(process.env.ENGINE_TOTAL || "100", 10) || 100,
  STRATEGY: process.env.ENGINE_STRATEGY || "auto",
  VELOCITY_RUNNER:
    process.env.VELOCITY_ENGINE ||
    path.resolve(process.cwd(), "target", "debug", "velocity-engine-runner.exe"),
};

const STRATEGIES = {
  velocity: {
    run: (cmd, id) => {
      const args = [CONFIG.MARKER, id, cmd];
      return spawn(CONFIG.VELOCITY_RUNNER, args, { shell: false });
    },
  },
  cmd: {
    wrap: (cmd, id) =>
      `${cmd} & echo changed directory to %cd% & echo ${CONFIG.MARKER}${id}__%errorlevel%__`,
    shell: "cmd.exe",
    args: (wrapped) => ["/c", wrapped],
    run: (cmd, id) => {
      const wrapped = STRATEGIES.cmd.wrap(cmd, id);
      return spawn(STRATEGIES.cmd.shell, STRATEGIES.cmd.args(wrapped), {
        shell: false,
      });
    },
  },
  posix: {
    wrap: (cmd, id) =>
      `${cmd}; printf 'changed directory to %s\\n' "$PWD"; printf '\\n${CONFIG.MARKER}${id}__%s__\\n' "$?"`,
    shell: "bash",
    args: (wrapped) => ["-c", wrapped],
    run: (cmd, id) => {
      const wrapped = STRATEGIES.posix.wrap(cmd, id);
      return spawn(STRATEGIES.posix.shell, STRATEGIES.posix.args(wrapped), {
        shell: false,
      });
    },
  },
};

function pickStrategy() {
  if (CONFIG.STRATEGY !== "auto") return CONFIG.STRATEGY;
  if (fs.existsSync(CONFIG.VELOCITY_RUNNER)) return "velocity";
  return process.platform === "win32" ? "cmd" : "posix";
}

class MarkerParser {
  carry = "";
  prefix;

  constructor(blockId) {
    this.prefix = `${CONFIG.MARKER}${blockId}__`;
  }

  consume(chunk) {
    const combined = this.carry + chunk;
    const markerStart = combined.lastIndexOf(this.prefix);

    if (markerStart >= 0) {
      const remainder = combined.slice(markerStart + this.prefix.length);
      const match = remainder.match(/^(-?\d+)__/);

      if (match) {
        const exitCode = Number.parseInt(match[1], 10);
        const cleaned = combined.slice(0, markerStart);
        this.carry = "";
        return { cleaned, exitCode };
      }
      this.carry = combined.slice(markerStart);
      return { cleaned: combined.slice(0, markerStart) };
    }

    if (combined.length > 1024) {
      this.carry = combined.slice(-this.prefix.length);
      return { cleaned: combined.slice(0, -this.prefix.length) };
    }

    this.carry = combined;
    return { cleaned: "" };
  }
}

function categoriesFor(strategyName) {
  const isWin = process.platform === "win32";
  const isVelocity = strategyName === "velocity";

  // Velocity uses the app's shell behavior, which is PowerShell on Windows.
  if (isWin && isVelocity) {
    return {
      SYSTEM: [
        "Write-Output Engine Check",
        "$env:USERNAME",
        "$env:COMPUTERNAME",
        "$PSVersionTable.PSVersion.ToString()",
        "Get-Location | Select-Object -ExpandProperty Path",
        "Get-Item Env:PATH | Select-Object -ExpandProperty Value",
      ],
      DEV: [
        "node -v",
        "npm -v",
        "git --version",
        "git status",
        "cargo --version",
      ],
      SANDBOX: [
        `Get-ChildItem -LiteralPath "${CONFIG.SANDBOX}" -Force | Select-Object -First 5 | ForEach-Object { $_.Name }`,
        `Set-Location -LiteralPath "${CONFIG.SANDBOX}"; Write-Output in_sandbox`,
      ],
      LOGIC: [
        'Get-ChildItem -Filter "*.json" -ErrorAction SilentlyContinue | Select-Object -First 1 | ForEach-Object { $_.Name }',
        "Write-Output 1; Write-Output 2",
        'if (!(Test-Path -LiteralPath "non_existent")) { Write-Output fallback }',
      ],
    };
  }

  return {
    SYSTEM: [
      "echo Engine Check",
      "whoami",
      "hostname",
      isWin ? "ver" : "uname -a",
      "cd",
      isWin ? "set" : "env",
    ],
    DEV: ["node -v", "npm -v", "git --version", "git status", "cargo --version"],
    SANDBOX: [
      isWin ? `dir "${CONFIG.SANDBOX}"` : `ls -la "${CONFIG.SANDBOX}"`,
      isWin ? `cd "${CONFIG.SANDBOX}"; echo in_sandbox` : `cd "${CONFIG.SANDBOX}" && echo in_sandbox`,
    ],
    LOGIC: [
      isWin ? 'dir | findstr ".json"' : 'ls | grep ".json" || true',
      "echo 1 && echo 2",
      isWin ? "dir non_existent || echo fallback" : "ls non_existent || echo fallback",
    ],
  };
}

async function runTest(strategyName, command, category) {
  const blockId = Math.random().toString(36).slice(2, 8);
  const start = Date.now();
  const child = STRATEGIES[strategyName].run(command, blockId);
  const parser = new MarkerParser(blockId);

  let fullOutput = "";
  let exitCode = undefined;

  return await new Promise((resolve) => {
    child.stdout?.on("data", (d) => {
      const parsed = parser.consume(d.toString());
      if (parsed.cleaned) fullOutput += parsed.cleaned;
      if (parsed.exitCode !== undefined) exitCode = parsed.exitCode;
    });

    child.stderr?.on("data", (d) => {
      fullOutput += d.toString();
    });

    child.on("close", (code) => {
      const durationMs = Date.now() - start;
      if (exitCode === undefined) exitCode = code ?? undefined;

      let grade = 100;
      let notes = "Passed";

      if (fullOutput.includes(CONFIG.MARKER)) {
        grade -= 50;
        notes = "Marker Leak";
      }
      if (exitCode !== 0 && !command.includes("||")) {
        grade -= 10;
        notes = `Exit ${exitCode}`;
      }

      resolve({
        command,
        category,
        exitCode,
        durationMs,
        grade,
        notes,
        output: fullOutput,
      });
    });
  });
}

async function main() {
  if (!fs.existsSync(CONFIG.SANDBOX))
    fs.mkdirSync(CONFIG.SANDBOX, { recursive: true });
  if (!fs.existsSync(path.dirname(CONFIG.REPORT_PATH)))
    fs.mkdirSync(path.dirname(CONFIG.REPORT_PATH), { recursive: true });

  const strategyName = pickStrategy();
  if (!STRATEGIES[strategyName]) {
    throw new Error(`Unknown strategy: ${strategyName}`);
  }
  if (strategyName === "velocity" && !fs.existsSync(CONFIG.VELOCITY_RUNNER)) {
    throw new Error(
      `Velocity runner not found at ${CONFIG.VELOCITY_RUNNER}. Build it with: cargo build --bin velocity-engine-runner`
    );
  }

  const categories = categoriesFor(strategyName);
  const tests = [];
  for (const [cat, cmds] of Object.entries(categories)) {
    for (const cmd of cmds) tests.push({ cmd, cat });
  }
  while (tests.length < CONFIG.TOTAL_COMMANDS)
    tests.push({ cmd: `echo "Fill ${tests.length}"`, cat: "FILL" });

  process.stdout.write(
    `\x1b[36m>>> Starting Engine Benchmark [Strategy: ${strategyName}] [Marker: ${CONFIG.MARKER}]\x1b[0m\n`
  );

  const results = [];
  for (const test of tests) {
    const res = await runTest(strategyName, test.cmd, test.cat);
    results.push(res);
    process.stdout.write(res.grade === 100 ? "\x1b[32m.\x1b[0m" : "\x1b[31mF\x1b[0m");
  }

  const avgGrade = results.reduce((a, b) => a + b.grade, 0) / results.length;
  const avgTime = results.reduce((a, b) => a + b.durationMs, 0) / results.length;

  const resultCategories = [...new Set(results.map((r) => r.category))];
  const report = `# Engine Benchmark Report

Generated on: ${new Date().toISOString()}
Strategy: \`${strategyName}\`
Target Marker: \`${CONFIG.MARKER}\`

## Metrics
- **Grade:** ${avgGrade.toFixed(1)}/100
- **Latency:** ${avgTime.toFixed(1)}ms
- **Leaks:** ${results.filter((r) => r.output.includes(CONFIG.MARKER)).length}

## Category Breakdown
| Category | Avg Grade | Avg Time |
|----------|-----------|----------|
${resultCategories
  .map((cat) => {
    const r = results.filter((x) => x.category === cat);
    const cg = r.reduce((a, b) => a + b.grade, 0) / r.length;
    const ct = r.reduce((a, b) => a + b.durationMs, 0) / r.length;
    return `| ${cat} | ${cg.toFixed(1)} | ${ct.toFixed(1)}ms |`;
  })
  .join("\n")}

## Samples
| Command | Exit | Time | Grade | Notes |
|---------|------|------|-------|-------|
${results
  .slice(0, 20)
  .map(
    (r) =>
      `| \`${r.command}\` | ${r.exitCode} | ${r.durationMs}ms | ${r.grade} | ${r.notes} |`
  )
  .join("\n")}

**Conclusion:** ${avgGrade > 90 ? "Engine is robust." : "Engine needs tuning."}
`;

  fs.writeFileSync(CONFIG.REPORT_PATH, report);
  process.stdout.write(`\n\n\x1b[32mReport saved to: ${CONFIG.REPORT_PATH}\x1b[0m\n`);
}

main().catch((err) => {
  console.error(err);
  process.exitCode = 1;
});
