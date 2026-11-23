import { createMemo, createResource, createSignal, For, onCleanup, onMount, Show } from "solid-js";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import { open } from "@tauri-apps/plugin-dialog";
import { isPointOver } from "soulwindow/runtime";
import {
  Banner,
  Button,
  GroupBox,
  Icon,
  IconText,
  InputField,
  ProgressBar,
  SegmentedSwitch,
  Select,
  SettingsRow,
  SettingsToggleRow,
  StatCard,
} from "soulwindow/solid";
import { api, type FileError, type Mode, type Options, type Status } from "./api";
import { sw } from "./theme";

type Target = "folder" | "inPlace";

const EFFORTS = [
  { value: "0", label: "0 · Fastest" },
  { value: "1", label: "1 · Smaller, several times slower" },
  ...[2, 3, 4, 5, 6, 7, 8].map((n) => ({ value: String(n), label: String(n) })),
  { value: "9", label: "9 · Smallest, very slow" },
] as const;

const MiB = 1024 * 1024;

function formatBytes(bytes: number): string {
  return bytes >= 1024 * MiB ? `${(bytes / (1024 * MiB)).toFixed(2)} GiB` : `${(bytes / MiB).toFixed(1)} MiB`;
}

function formatDuration(ms: number): string {
  const s = Math.round(ms / 1000);
  const h = Math.floor(s / 3600);
  const m = Math.floor((s % 3600) / 60);
  return h > 0 ? `${h}h ${m}m` : m > 0 ? `${m}m ${s % 60}s` : `${s}s`;
}

export function App() {
  const [input, setInput] = createSignal("");
  const [target, setTarget] = createSignal<Target>("folder");
  /** Empty: the default `<input>_webp`. */
  const [output, setOutput] = createSignal("");
  const [mode, setMode] = createSignal<Mode>("lossless");
  const [effort, setEffort] = createSignal(0);
  const [nearLossless, setNearLossless] = createSignal(60);
  const [quality, setQuality] = createSignal(90);
  const [recursive, setRecursive] = createSignal(true);
  const [keepSmaller, setKeepSmaller] = createSignal(true);
  const [overwrite, setOverwrite] = createSignal(false);

  const [status, setStatus] = createSignal<Status | null>(null);
  const [errors, setErrors] = createSignal<FileError[]>([]);
  const [startError, setStartError] = createSignal<string | null>(null);
  const [dragging, setDragging] = createSignal(false);
  const running = () => status()?.running ?? false;

  const [defaultOutput] = createResource(input, (dir) => (dir ? api.defaultOutput(dir) : Promise.resolve("")));

  let mainEl!: HTMLElement;
  let timer: number | undefined;

  async function poll() {
    const s = await api.status(errors().length);
    if (!s) return;
    if (s.errors.length) setErrors((e) => [...e, ...s.errors]);
    setStatus(s);
    if (!s.running) {
      window.clearInterval(timer);
      timer = undefined;
    }
  }

  async function start() {
    setStartError(null);
    const dir = input().trim();
    if (!dir) {
      setStartError("Choose a folder first.");
      return;
    }
    const options: Options = {
      input: dir,
      output: target() === "inPlace" ? null : output().trim() || (await api.defaultOutput(dir)),
      encoding: { mode: mode(), effort: effort(), nearLossless: nearLossless(), quality: quality() },
      recursive: recursive(),
      keepSmallerOriginal: keepSmaller(),
      overwrite: overwrite(),
      threads: 0,
    };
    try {
      await api.start(options);
    } catch (e) {
      setStartError(String(e));
      return;
    }
    setErrors([]);
    setStatus(null);
    timer = window.setInterval(() => void poll(), 250);
    void poll();
  }

  async function pickFolder(set: (dir: string) => void, current: string) {
    const dir = await open({ directory: true, defaultPath: current || undefined });
    if (typeof dir === "string") set(dir);
  }

  onMount(() => {
    const unlisten = getCurrentWebview().onDragDropEvent((ev) => {
      const p = ev.payload;
      if (p.type === "enter" || p.type === "over") {
        setDragging(!running() && isPointOver(mainEl, p.position));
      } else {
        setDragging(false);
        const first = p.type === "drop" ? p.paths[0] : undefined;
        if (first && !running()) void api.folderOf(first).then(setInput);
      }
    });
    onCleanup(() => {
      window.clearInterval(timer);
      void unlisten.then((f) => f());
    });
  });

  const stats = createMemo(() => {
    const s = status();
    if (!s) return null;
    const p = s.progress;
    const secs = s.elapsedMs / 1000;
    const rate = secs > 0 ? p.done / secs : 0;
    const left = p.found - p.done;
    return {
      ...p,
      fraction: p.found > 0 ? p.done / p.found : 0,
      rate,
      saved: p.bytesIn - p.bytesOut,
      savedPct: p.bytesIn > 0 ? (100 * (p.bytesIn - p.bytesOut)) / p.bytesIn : 0,
      eta: s.running && !p.scanning && rate > 0 && left > 0 ? formatDuration((left / rate) * 1000) : null,
    };
  });

  return (
    <div class="bw-app">
      <header class="bw-header">
        <IconText icon={<Icon name="images" />}>batchWEBP</IconText>
        <Button icon={<Icon name="circle-half" />} title="Theme" aria-label="Theme" onClick={() => sw.cycleTheme()} />
      </header>

      <main ref={mainEl} class="bw-main" classList={{ "ds-drop-active": dragging() }}>
        <fieldset class="bw-fieldset" disabled={running()}>
          <GroupBox title={<IconText icon={<Icon name="folder2-open" />}>Source</IconText>}>
            <div class="bw-row">
              <InputField
                wrapperClass="bw-grow"
                value={input()}
                placeholder="Folder with images (or drop one here)"
                onInput={setInput}
              />
              <Button text="Browse" icon={<Icon name="folder" />} onClick={() => void pickFolder(setInput, input())} />
            </div>
            <SettingsToggleRow label="Include subfolders" checked={recursive()} onChange={setRecursive} divider />
          </GroupBox>

          <GroupBox title={<IconText icon={<Icon name="box-arrow-right" />}>Output</IconText>}>
            <SegmentedSwitch<Target>
              fill
              value={target()}
              onChange={setTarget}
              options={[
                { value: "folder", text: "Output folder", icon: <Icon name="folder-plus" /> },
                { value: "inPlace", text: "In place", icon: <Icon name="arrow-repeat" /> },
              ]}
            />
            <Show
              when={target() === "folder"}
              fallback={
                <Banner tone="warn">
                  Each original is deleted once its WebP is written. Originals that would not shrink stay untouched.
                </Banner>
              }
            >
              <div class="bw-row">
                <InputField
                  wrapperClass="bw-grow"
                  value={output()}
                  placeholder={defaultOutput() || "<source>_webp next to the source"}
                  onInput={setOutput}
                />
                <Button text="Browse" icon={<Icon name="folder" />} onClick={() => void pickFolder(setOutput, output())} />
              </div>
              <SettingsToggleRow
                label="Replace existing files"
                desc="Off: files already in the output folder are skipped, so a stopped run resumes."
                checked={overwrite()}
                onChange={setOverwrite}
              />
            </Show>
          </GroupBox>

          <GroupBox title={<IconText icon={<Icon name="sliders" />}>Encoding</IconText>}>
            <SegmentedSwitch<Mode>
              fill
              value={mode()}
              onChange={setMode}
              options={[
                { value: "lossless", text: "Lossless" },
                { value: "nearLossless", text: "Near-lossless" },
                { value: "lossy", text: "Lossy" },
              ]}
            />
            <SettingsRow
              label="Effort"
              desc={
                mode() === "nearLossless" && effort() === 0
                  ? "Near-lossless needs effort 1 or more; 1 is used."
                  : "Higher: smaller files, slower."
              }
            >
              <Select value={String(effort())} options={EFFORTS} onChange={(v) => setEffort(Number(v))} />
            </SettingsRow>
            <Show when={mode() === "nearLossless"}>
              <SettingsRow label={`Strength ${nearLossless()}`} desc="Lower: smaller files, more change. 100 is lossless." divider>
                <input
                  type="range"
                  min="0"
                  max="100"
                  step="20"
                  value={nearLossless()}
                  onInput={(e) => setNearLossless(Number(e.currentTarget.value))}
                />
              </SettingsRow>
            </Show>
            <Show when={mode() === "lossy"}>
              <SettingsRow label={`Quality ${quality()}`} divider>
                <input type="range" min="50" max="100" value={quality()} onInput={(e) => setQuality(Number(e.currentTarget.value))} />
              </SettingsRow>
            </Show>
            <SettingsToggleRow
              label="Keep original if WebP isn't smaller"
              desc="Common for JPEGs in lossless mode. In an output folder the original is copied instead."
              checked={keepSmaller()}
              onChange={setKeepSmaller}
              divider
            />
          </GroupBox>
        </fieldset>

        <div class="bw-row bw-actions">
          <Show
            when={running()}
            fallback={<Button variant="primary" text="Convert" icon={<Icon name="play-fill" />} onClick={() => void start()} />}
          >
            <Button
              variant="danger"
              text={status()?.cancelled ? "Stopping" : "Stop"}
              loading={status()?.cancelled}
              icon={<Icon name="stop-fill" />}
              onClick={() => void api.cancel()}
            />
          </Show>
          <ProgressBar class="bw-grow" value={running() && !stats()?.found ? undefined : (stats()?.fraction ?? 0)} />
        </div>

        <Show when={startError()}>{(msg) => <Banner tone="error">{msg()}</Banner>}</Show>
        <Show when={status()?.error}>{(msg) => <Banner tone="error">{msg()}</Banner>}</Show>
        <Show when={status() && !status()!.running && !status()!.error}>
          <Banner tone={status()!.cancelled ? "warn" : "ok"}>
            {status()!.cancelled ? "Stopped" : "Done"} in {formatDuration(status()!.elapsedMs)}.
          </Banner>
        </Show>

        <Show when={stats()}>
          {(s) => (
            <div class="bw-stats">
              <StatCard value={`${s().done.toLocaleString()} / ${s().found.toLocaleString()}${s().scanning ? "+" : ""}`} label="Files" />
              <StatCard value={`${s().rate.toFixed(s().rate < 100 ? 1 : 0)}/s`} label={s().eta ? `ETA ${s().eta}` : "Speed"} />
              <StatCard variant="success" value={`${s().savedPct.toFixed(1)}%`} label={`${formatBytes(s().saved)} saved`} />
              <StatCard variant="primary" value={s().converted.toLocaleString()} label="Converted" />
              <StatCard variant="neutral" value={(s().kept + s().skipped).toLocaleString()} label={`Kept ${s().kept} · Skipped ${s().skipped}`} />
              <StatCard variant={s().failed > 0 ? "danger" : "neutral"} value={s().failed.toLocaleString()} label="Failed" />
            </div>
          )}
        </Show>

        <Show when={errors().length > 0}>
          <GroupBox title={<IconText icon={<Icon name="exclamation-triangle" />}>Failures ({errors().length})</IconText>}>
            <div class="bw-errors">
              <For each={errors()}>
                {(e) => (
                  <div class="bw-error" title={e.path}>
                    <span class="bw-error-path">{e.path}</span>
                    <span class="bw-error-msg">{e.error}</span>
                  </div>
                )}
              </For>
            </div>
          </GroupBox>
        </Show>
      </main>
    </div>
  );
}
