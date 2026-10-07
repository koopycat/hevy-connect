const COMMON = `Common output flags:
  --format toon|json  Select output format (default: toon)
  --json              Alias for --format json
  --full              Return the complete wire payload
  --fields <paths>    Project comma-separated dot paths`;

const PAGINATION = `Pagination flags:
  --page <n>           Fetch page n
  --page-size <n>      Items per API page
  --limit <n>          Return at most n items; resume marks a cut page
  --all                Fetch all pages (max 500 pages / 5000 items)`;

const MUTATION = `Mutation flags:
  --file <path|->      JSON object file, or - for stdin
  --confirm            Required to send the mutation
  --dry-run            Validate and preview without an API request
  --full               Include the complete body/result`;

export const TOP_LEVEL_HELP = `hevy-axi — agent-friendly Hevy Public API CLI

Usage:
  hevy-axi <command> <action> [arguments] [flags]

Commands:
  user         info
  workout      list, count, events, view, create, update
  routine      list, view, create, update
  exercise     list, view, history, create
  folder       list, view, create
  measurement  list, view, create, update
  setup        status, key, remove-key, hooks, remove-hooks
  update       Report safe manual checkout update instructions

Note: axi-sdk's "built-in": update is shadowed because this package is private.
Run "hevy-axi <command> --help" for every action, flag, and example.`;

const HELP: Readonly<Record<string, string>> = {
  user: `Usage: hevy-axi user info [flags]

Actions:
  info                Show authenticated account information

${COMMON}

Examples:
  hevy-axi user info
  hevy-axi user info --fields username,weight_unit --json`,

  workout: `Usage: hevy-axi workout <action> [arguments] [flags]

Actions:
  list                List workouts
  count               Return the exact workout count
  events              List workout update/delete events
  view <id>           View one workout with each exercise's sets
  create              Create a workout from JSON
  update <id>         Fully replace a workout from JSON (all fields required)

${PAGINATION}
  --since <ISO>        Events since an ISO-8601 timestamp (events only)
${COMMON}
${MUTATION}

Examples:
  hevy-axi workout list --page-size 10
  hevy-axi workout list --all --limit 100 --json
  hevy-axi workout events --since 2024-01-01T00:00:00Z
  hevy-axi workout view <id> --full
  hevy-axi workout create --file workout.json --dry-run
  hevy-axi workout update <id> --file - --confirm`,

  routine: `Usage: hevy-axi routine <action> [arguments] [flags]

Actions:
  list                List routines
  view <id>           View one routine with each exercise's planned sets
  create              Create a routine from JSON
  update <id>         Fully replace a routine from JSON (all fields required)

${PAGINATION}
${COMMON}
${MUTATION}

Examples:
  hevy-axi routine list --all
  hevy-axi routine view <id>
  hevy-axi routine create --file routine.json --confirm
  hevy-axi routine update <id> --file - --dry-run --full`,

  exercise: `Usage: hevy-axi exercise <action> [arguments] [flags]

Actions:
  list                List exercise templates (page size 10, or 100 with --all)
  view <id>           View one exercise template
  history <id>        Show exercise set history (default output capped at 50)
  create              Create a custom exercise template

${PAGINATION}
  --start <ISO>        History start timestamp
  --end <ISO>          History end timestamp
${COMMON}
${MUTATION}

Examples:
  hevy-axi exercise list --page-size 100
  hevy-axi exercise view <id>
  hevy-axi exercise history <id> --start 2024-01-01T00:00:00Z
  hevy-axi exercise create --file exercise.json --confirm`,

  folder: `Usage: hevy-axi folder <action> [arguments] [flags]

Actions:
  list                List routine folders
  view <id>           View one routine folder
  create              Create at index 0, shifting all existing folders

${PAGINATION}
${COMMON}
${MUTATION}

Examples:
  hevy-axi folder list
  hevy-axi folder view 42
  hevy-axi folder create --file folder.json --dry-run`,

  measurement: `Usage: hevy-axi measurement <action> [arguments] [flags]

Actions:
  list                List body measurements
  view <YYYY-MM-DD>   View measurements for a date
  create              Create a date-keyed measurement
  update <YYYY-MM-DD> Safely patch values by merging with the current measurement

${PAGINATION}
${COMMON}
${MUTATION}

Examples:
  hevy-axi measurement list --all
  hevy-axi measurement view 2024-08-14
  hevy-axi measurement create --file measurement.json --confirm
  hevy-axi measurement update 2024-08-14 --file - --dry-run

Measurement updates accept only documented measurement keys with number|null
values. Confirmed updates read the current date, merge the patch, reject unknown
upstream fields, and PUT one complete replacement body without date. Dry-run
performs no API request and reports strategy: merge_with_current.`,

  update: `Usage: hevy-axi update [--check] [flags]

Report the installed version, the checkout it runs from, and the command that
updates it. This private checkout is not published to npm, so the command never
contacts npm or changes local files.

Flags:
  --check             Same read-only report; accepted for AXI compatibility
  --help              Show this help
${COMMON}

Update command, run in the reported checkout:
  git pull --ff-only && just install && just build`,

  setup: `Usage: hevy-axi setup <action> [flags]

Actions:
  status              Show credential category, base URL, and hook status
  key                 Read an API key only from stdin and store it globally
  remove-key          Remove the globally stored API key
  hooks               Install AXI SessionStart hooks
  remove-hooks        Remove AXI SessionStart hooks

Flags:
  --confirm            Required for key/remove-key/hooks/remove-hooks
${COMMON}

Examples:
  printf '%s\\n' "$HEVY_API_KEY" | hevy-axi setup key --confirm
  hevy-axi setup status --json
  hevy-axi setup remove-key --confirm
  hevy-axi setup hooks --confirm
  hevy-axi setup remove-hooks --confirm`,
};

export function commandHelp(command: string): string | undefined {
  return HELP[command];
}
