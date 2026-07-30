/** Tokenize a command line with quotes and backslash escapes. */
export function tokenize(input: string): string[] {
  const tokens: string[] = []
  let current = ''
  let quote: '"' | "'" | null = null
  let escaped = false

  for (const char of input) {
    if (escaped) {
      current += char
      escaped = false
      continue
    }
    if (char === '\\' && quote !== "'") {
      escaped = true
      continue
    }
    if (quote) {
      if (char === quote) quote = null
      else current += char
      continue
    }
    if (char === '"' || char === "'") {
      quote = char
      continue
    }
    if (/\s/.test(char)) {
      if (current.length > 0) {
        tokens.push(current)
        current = ''
      }
      continue
    }
    current += char
  }
  if (escaped) current += '\\'
  if (current.length > 0) tokens.push(current)
  return tokens
}

export type ParsedCommand =
  | { cmd: 'pwd' }
  | { cmd: 'ls'; path?: string }
  | { cmd: 'cd'; path: string }
  | { cmd: 'mkdir'; folderName: string }
  | { cmd: 'mv'; source: string; dest: string }
  | { cmd: 'rm'; target: string; force: boolean }
  | { cmd: 'restore'; target: string }
  | { cmd: 'open'; target: string }

export function parseCommand(input: string): ParsedCommand | { error: string } {
  const tokens = tokenize(input.trim())
  if (tokens.length === 0) return { error: 'Empty command' }

  const [name, ...args] = tokens
  switch (name) {
    case 'pwd':
      if (args.length > 0) return { error: 'pwd takes no arguments' }
      return { cmd: 'pwd' }
    case 'ls':
      if (args.length > 1) return { error: 'usage: ls [path]' }
      return { cmd: 'ls', path: args[0] }
    case 'cd':
      if (args.length !== 1) return { error: 'usage: cd <path>' }
      return { cmd: 'cd', path: args[0] }
    case 'mkdir':
      if (args.length !== 1) return { error: 'usage: mkdir <name>' }
      return { cmd: 'mkdir', folderName: args[0] }
    case 'mv':
      if (args.length !== 2) return { error: 'usage: mv <source> <dest>' }
      return { cmd: 'mv', source: args[0], dest: args[1] }
    case 'rm': {
      const force = args.includes('-f') || args.includes('--force')
      const targets = args.filter((arg) => arg !== '-f' && arg !== '--force')
      if (targets.length !== 1) return { error: 'usage: rm [-f|--force] <target>' }
      return { cmd: 'rm', target: targets[0], force }
    }
    case 'restore':
      if (args.length !== 1) return { error: 'usage: restore <target>' }
      return { cmd: 'restore', target: args[0] }
    case 'open':
      if (args.length !== 1) return { error: 'usage: open <target>' }
      return { cmd: 'open', target: args[0] }
    default:
      return { error: `Unknown command: ${name}` }
  }
}

export function splitPath(path: string): string[] {
  return path
    .split('/')
    .map((segment) => segment.trim())
    .filter((segment) => segment.length > 0 && segment !== '.')
}
