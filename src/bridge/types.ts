import type { AdvisorStatus } from '../bindings/AdvisorStatus';
import type { CategoryFreed } from '../bindings/CategoryFreed';
import type { CategoryId } from '../bindings/CategoryId';
import type { CategoryTotal } from '../bindings/CategoryTotal';
import type { DriveKind } from '../bindings/DriveKind';
import type { FullDiskAccess } from '../bindings/FullDiskAccess';
import type { ItemKind } from '../bindings/ItemKind';
import type { LeftForYou } from '../bindings/LeftForYou';
import type { MapCategory } from '../bindings/MapCategory';
import type { MapFolder } from '../bindings/MapFolder';
import type { MapItem } from '../bindings/MapItem';
import type { Outcome } from '../bindings/Outcome';
import type { Overview } from '../bindings/Overview';
import type { PlanStep } from '../bindings/PlanStep';
import type { PlanSummary } from '../bindings/PlanSummary';
import type { Proposal } from '../bindings/Proposal';
import type { ProposalAction } from '../bindings/ProposalAction';
import type { ProposalSource } from '../bindings/ProposalSource';
import type { Reorganized } from '../bindings/Reorganized';
import type { Risk } from '../bindings/Risk';
import type { RunEvent } from '../bindings/RunEvent';
import type { RunHistory } from '../bindings/RunHistory';
import type { RunListing } from '../bindings/RunListing';
import type { RunReport } from '../bindings/RunReport';
import type { ScanOutcome } from '../bindings/ScanOutcome';
import type { ScanSummary } from '../bindings/ScanSummary';
import type { ScanUpdate } from '../bindings/ScanUpdate';
import type { Skip } from '../bindings/Skip';
import type { SkipKind } from '../bindings/SkipKind';
import type { StepReport } from '../bindings/StepReport';
import type { VolumeInfo } from '../bindings/VolumeInfo';
import type { GuideId } from '../bindings/GuideId';
import type { FastFallback } from '../bindings/FastFallback';
import type { FastStage } from '../bindings/FastStage';
import type { IndexRead } from '../bindings/IndexRead';
import type { LockedGroup } from '../bindings/LockedGroup';
import type { LockedKind } from '../bindings/LockedKind';
import type { ScanMethod } from '../bindings/ScanMethod';
import type { DuplicateCopy } from '../bindings/DuplicateCopy';
import type { DuplicateGroup } from '../bindings/DuplicateGroup';
import type { DuplicatePair } from '../bindings/DuplicatePair';
import type { DuplicateProgress } from '../bindings/DuplicateProgress';
import type { DuplicateStage } from '../bindings/DuplicateStage';
import type { Duplicates } from '../bindings/Duplicates';
import type { DuplicatesDone } from '../bindings/DuplicatesDone';
import type { DuplicatesOutcome } from '../bindings/DuplicatesOutcome';
import type { FileKind } from '../bindings/FileKind';
import type { PlannedDuplicates } from '../bindings/PlannedDuplicates';
import type { PlannedTile } from '../bindings/PlannedTile';
import type { TileFreed } from '../bindings/TileFreed';
import type { MoveProgress } from '../bindings/MoveProgress';
import type { StepKind } from '../bindings/StepKind';
import type { TidyFolder } from '../bindings/TidyFolder';
import type { TidyKind } from '../bindings/TidyKind';
import type { TidyListing } from '../bindings/TidyListing';
import type { TidyMove } from '../bindings/TidyMove';
import type { TidyProposal } from '../bindings/TidyProposal';
import type { TidyStay } from '../bindings/TidyStay';
import type { TidySubfolder } from '../bindings/TidySubfolder';
import type { Undone } from '../bindings/Undone';
import type { ClaudeAbout } from '../bindings/ClaudeAbout';
import type { ClaudeAnswer } from '../bindings/ClaudeAnswer';
import type { ClaudeBasis } from '../bindings/ClaudeBasis';
import type { ClaudeChange } from '../bindings/ClaudeChange';
import type { ClaudeChangeKind } from '../bindings/ClaudeChangeKind';
import type { ClaudeCheck } from '../bindings/ClaudeCheck';
import type { ClaudeDropped } from '../bindings/ClaudeDropped';
import type { ClaudeEffort } from '../bindings/ClaudeEffort';
import type { ClaudeEvent } from '../bindings/ClaudeEvent';
import type { ClaudeFailure } from '../bindings/ClaudeFailure';
import type { ClaudeFine } from '../bindings/ClaudeFine';
import type { ClaudeFlag } from '../bindings/ClaudeFlag';
import type { ClaudeFolderNote } from '../bindings/ClaudeFolderNote';
import type { ClaudeHidden } from '../bindings/ClaudeHidden';
import type { ClaudeHiddenGroup } from '../bindings/ClaudeHiddenGroup';
import type { ClaudeJob } from '../bindings/ClaudeJob';
import type { ClaudeJobKind } from '../bindings/ClaudeJobKind';
import type { ClaudeMade } from '../bindings/ClaudeMade';
import type { ClaudeModel } from '../bindings/ClaudeModel';
import type { ClaudeChatStarted } from '../bindings/ClaudeChatStarted';
import type { ClaudeRefreshed } from '../bindings/ClaudeRefreshed';
import type { ClaudeResult } from '../bindings/ClaudeResult';
import type { ClaudeState } from '../bindings/ClaudeState';
import type { ClaudeStatus } from '../bindings/ClaudeStatus';
import type { TidyLayout } from '../bindings/TidyLayout';

export type {
  AdvisorStatus,
  CategoryFreed,
  CategoryId,
  CategoryTotal,
  DriveKind,
  DuplicateCopy,
  DuplicateGroup,
  DuplicatePair,
  DuplicateProgress,
  DuplicateStage,
  Duplicates,
  DuplicatesDone,
  DuplicatesOutcome,
  FastFallback,
  FastStage,
  FileKind,
  FullDiskAccess,
  IndexRead,
  ItemKind,
  LeftForYou,
  LockedGroup,
  LockedKind,
  MapCategory,
  MapFolder,
  MapItem,
  Outcome,
  Overview,
  PlanStep,
  PlanSummary,
  PlannedDuplicates,
  PlannedTile,
  Proposal,
  ProposalAction,
  ProposalSource,
  Reorganized,
  Risk,
  RunEvent,
  RunHistory,
  RunListing,
  RunReport,
  ScanMethod,
  ScanOutcome,
  ScanSummary,
  ScanUpdate,
  Skip,
  SkipKind,
  StepReport,
  TileFreed,
  VolumeInfo,
  MoveProgress,
  StepKind,
  TidyFolder,
  TidyKind,
  TidyListing,
  TidyMove,
  TidyProposal,
  TidyStay,
  TidySubfolder,
  Undone,
  ClaudeAbout,
  ClaudeAnswer,
  ClaudeBasis,
  ClaudeChange,
  ClaudeChangeKind,
  ClaudeDropped,
  ClaudeEffort,
  ClaudeEvent,
  ClaudeFailure,
  ClaudeFine,
  ClaudeFlag,
  ClaudeFolderNote,
  ClaudeHidden,
  ClaudeHiddenGroup,
  ClaudeJob,
  ClaudeJobKind,
  ClaudeMade,
  ClaudeModel,
  ClaudeChatStarted,
  ClaudeRefreshed,
  ClaudeResult,
  ClaudeCheck,
  ClaudeState,
  ClaudeStatus,
  TidyLayout,
};

/** Which desktop the UI is running on. It decides the window chrome (macOS draws traffic lights over the header). */
export type Platform = 'windows' | 'macos';

/**
 * Everything the UI asks of the Rust core. Components reach it only through `useBridge()`, never through Tauri
 * directly, so the same UI runs against the real app or against fixture data in a plain browser and in tests.
 *
 * Data types come from Rust (`src/bindings`, generated by ts-rs); never write them by hand.
 */
export interface Bridge {
  readonly platform: Platform;
  /** This computer's name, as the OS shows it. */
  machineName(): Promise<string>;
  /** Passive or synthetic availability only. No CLI runs, no executable paths, AI always disabled. */
  advisorStatus(): Promise<AdvisorStatus>;
  /**
   * The drives to offer for scanning: internal, USB and external ones, the system drive first. Read-only and fast,
   * so the UI asks again now and then to notice drives coming and going. Rejects if the drives can't be read.
   */
  listVolumes(): Promise<VolumeInfo[]>;
  /** Forgets a drive that has gone away: its map, and its scan if one is running. */
  forgetDrive(mountPoint: string): Promise<void>;
  /** Whether macOS lets Vacancy see everything ("notApplicable" on Windows). */
  fullDiskAccess(): Promise<FullDiskAccess>;
  /** Opens System Settings at Full Disk Access (macOS). */
  openFullDiskAccessSettings(): Promise<void>;
  /**
   * Scans a volume, read-only. `onUpdate` gets the HUD's numbers about ten times a second, now and then with a
   * preview of the categories. Resolves with the map and the cleanups the rules propose (each with its own tile on
   * the map), or with `{ status: 'cancelled' }` after `cancelScan`. Each drive keeps its own map and proposals; one
   * scan runs at a time, so starting one cancels the scan running on any drive.
   *
   * With `fast` (Windows), it asks for a fast scan: Windows shows its admin prompt (`stage: 'asking'`), and a small
   * read-only helper reads the drive's file index (`'reading'`) before the core sorts it into categories
   * (`'sorting'`). Admin is used for that read only. Where it can't (the prompt is declined, the drive isn't NTFS,
   * the helper doesn't start, the index can't be read) the normal scan runs, every update and the map saying why in
   * `fallback`. Nothing is remembered: each fast scan asks again.
   */
  scan(mountPoint: string, onUpdate: (update: ScanUpdate) => void, options?: { fast?: boolean }): Promise<ScanOutcome>;
  /** Stops the running scan, on whichever drive. */
  cancelScan(): Promise<void>;
  /** Opens a folder tile of a drive's map. */
  browse(mountPoint: string, id: string): Promise<MapFolder>;
  /**
   * Looks for duplicate files in a drive's last scan, in the background, while the map is in use: reads only the
   * files that might be copies (to hash them, never to show them), `onProgress` hearing how far it has got about ten
   * times a second. Resolves with the map and proposals again, now with the "Duplicate files" proposal and its tiles
   * (one per personal folder holding extra copies) and the groups found; or `{ status: 'cancelled' }` if a new scan,
   * a run or the drive going away stopped it.
   */
  findDuplicates(mountPoint: string, onProgress: (progress: DuplicateProgress) => void): Promise<DuplicatesOutcome>;
  /**
   * Turns the proposals `ids` of a drive's last scan into a plan, for the confirm dialog: what it deletes, from how
   * many places, step by step. Every place passes the Guard again; the ones it refuses (or finds changed since the
   * scan) are left out of the numbers and listed in `refused`. Rejects if the drive has no scan to plan from (or
   * its scan already ran a plan: scan again first).
   */
  buildPlan(mountPoint: string, ids: string[], skippedGroups?: string[]): Promise<PlanSummary>;
  /**
   * Runs the plan `planId` of a drive: deletes permanently what it holds, re-checking every place right before it
   * goes. `onEvent` gets the run as it happens: steps starting and finishing, progress about ten times a second,
   * each place deleted, and each thing skipped with its reason. Resolves with the report, which is also kept in
   * the run history. One run at a time; no scan can start while it runs.
   */
  runPlan(mountPoint: string, planId: string, onEvent: (event: RunEvent) => void): Promise<RunReport>;
  /** Stops the running plan after the file being deleted. Its report says what was done and what was left. */
  cancelRun(): Promise<void>;
  /**
   * The run history: every kept run, newest first, and how many kept runs couldn't be read (they are passed over).
   * Never rejects for a damaged history file.
   */
  listRuns(): Promise<RunHistory>;
  /**
   * A kept run: everything its highlight slides and summary show. Each folder it tidied is read again from its
   * journal (what moved, whether Undo can still run, whether the run was interrupted); nothing is resumed.
   */
  openRun(id: string): Promise<RunReport>;
  /**
   * Undoes one folder's Tidy up (`tidyId`, like `tidy-downloads`) from the kept run `runId`, from its journal: every
   * moved file that is still as it was left goes back, and the empty folders the run made are removed. Resolves
   * with the run's report, updated with what came back, what couldn't and why. Windows only; it works after a
   * restart too. Rejects while a plan or scan runs.
   */
  undoTidy(runId: string, tidyId: string): Promise<RunReport>;
  /** Opens a trusted numeric Steam hint from a retained scan. Never accepts a URL or runs as part of a plan. */
  openHint(mountPoint: string, proposalId: string): Promise<void>;
  /** Opens a compiled vendor reference, including from history with no current scan. */
  openGuide(guide: GuideId): Promise<void>;

  // Live Claude (ADR 0008), through the user's own Claude Code. Claude proposes; it never executes. Everything it adds
  // is a proposal with `source: 'claude'`, unticked, risk Review, that passed the Guard; `dropped` names what didn't.

  /**
   * Claude's status: `notFound` (no Claude Code on this computer), `off` (the default: nothing is sent), `signedOut`
   * (on, but Claude Code isn't signed in) or `on`; privacy mode; whether this is fixture mode (reads confined to the
   * fixture); and what always stays hidden, for the privacy screens. Looks for Claude Code again each time.
   */
  claudeStatus(): Promise<ClaudeStatus>;
  /** Turns Claude on (after the privacy notice) or off. Off stops running requests; Claude's items stay in the plan. */
  claudeSetEnabled(on: boolean): Promise<ClaudeStatus>;
  /** Turns privacy mode on or off (confirm before turning it off). Remembered, also while Claude is off. */
  claudeSetPrivacy(on: boolean): Promise<ClaudeStatus>;
  /**
   * Picks the model (`default`: no `--model`, the account's) and reasoning (`default`: no `--effort`) new questions
   * ask for. Remembered, also while Claude is off. It ends no chat; a request already running keeps its own.
   */
  claudeSetModel(model: ClaudeModel, effort: ClaudeEffort): Promise<ClaudeStatus>;
  /**
   * Checks this Claude Code before privacy mode off lets Claude open files (`check` in the status): one small real
   * call that reads a file Vacancy made and is refused one Vacancy hides. Kept per Claude Code (its path, size,
   * modified time and version), so it runs when the check is due, or always with `again` (Check again). Only with
   * Claude on and privacy mode off. Until it passes, every call runs as if privacy mode were on.
   */
  claudeCheck(again: boolean): Promise<ClaudeStatus>;
  // Every command below that touches a drive's scan names it: `scan` is the id its map came with (`ScanOutcome`'s
  // `scan`). Once the drive has a newer scan (or its plan ran, or it went away) the command is refused and changes
  // nothing: an item id like `claude-d17` means another item in another scan.

  /**
   * Starts a job on the drive's scan `scan` and resolves with its request id at once. `onEvent` then hears `working`,
   * `slow` (still running after 20 s), and `done` with the checked answer or `failed` with why (`notFound`,
   * `signedOut`, `timedOut` after 180 s, `cancelled`, `chatEnded`, or `error` with Claude's text, such as a rate
   * limit). A `done` answer that changed the map carries `refreshed`: the overview, proposals and Tidy up sorts to
   * show from now on.
   */
  claudeStart(mountPoint: string, scan: number, job: ClaudeJob, onEvent: (event: ClaudeEvent) => void): Promise<string>;
  /**
   * Asks the person's question `text` about one proposal or folder, or the whole scan (`{ kind: 'scan' }`: it only
   * answers, and adds nothing to the plan), in the chat `conversation`, or with `null` in a
   * new chat (begun in the privacy mode now). The core keeps every chat and sends its turns so far itself; the UI
   * sends only the new question. Resolves with the request id and the chat's id. A chat that a privacy change (or
   * Claude turned off) ended answers `failed` with `chatEnded`, having sent nothing: ask again in a new chat.
   */
  claudeChat(
    mountPoint: string,
    scan: number,
    conversation: string | null,
    about: ClaudeAbout,
    text: string,
    onEvent: (event: ClaudeEvent) => void,
  ): Promise<ClaudeChatStarted>;
  /** Stops a running request (its Claude Code is killed); it ends `failed` with `cancelled`. */
  claudeCancel(requestId: string): Promise<void>;
  /**
   * Picks the layout a Tidy up sort (`tidy-downloads`...) uses from now on: `builtin`, or `claude` once Claude has
   * suggested one (a `tidyLayout` answer). Resolves with the map, proposals and sorts again. Windows only.
   */
  claudeChooseLayout(mountPoint: string, scan: number, tidyId: string, layout: TidyLayout): Promise<ClaudeRefreshed>;
  /** Takes Claude's items `ids` back out of the drive's scan `scan` (Keep mine). Rejects for a rule's item. */
  claudeRemoveItems(mountPoint: string, scan: number, ids: string[]): Promise<ClaudeRefreshed>;
}
