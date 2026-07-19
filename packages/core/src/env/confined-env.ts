import path from "node:path";

import {
  err,
  ExecutionError,
  FileError,
  ok,
  type ExecutionEnv,
  type FileInfo,
  type Result,
  type ShellExecOptions,
} from "@earendil-works/pi-agent-core";

/**
 * ExecutionEnv wrapper that confines every path-based filesystem operation to
 * a root directory (typically the session cwd). Relative inputs resolve
 * against the root; anything resolving outside the root is rejected with a
 * `permission_denied` FileError instead of touching the host fs.
 *
 * Shell commands run with `cwd` forced inside the root.
 *
 * Known gap (deferred past P1): confinement is purely syntactic. A symlink
 * inside the root can still point outside it. `canonicalPath` exists so a
 * later hardening pass can resolve-and-recheck real paths before reads and
 * writes.
 */
export class ConfinedExecutionEnv implements ExecutionEnv {
  private readonly innerEnv: ExecutionEnv;
  private readonly rootPath: string;

  constructor(inner: ExecutionEnv, root: string) {
    this.innerEnv = inner;
    this.rootPath = path.resolve(root);
  }

  get cwd(): string {
    return this.rootPath;
  }

  /** The confinement root, syntactically normalized. */
  get root(): string {
    return this.rootPath;
  }

  /** The wrapped environment, e.g. for components that own out-of-root paths (session repo). */
  get inner(): ExecutionEnv {
    return this.innerEnv;
  }

  private confine(input: string): Result<string, FileError> {
    const resolved = path.resolve(this.rootPath, input);
    const prefix = this.rootPath === path.sep ? path.sep : this.rootPath + path.sep;
    if (resolved === this.rootPath || resolved.startsWith(prefix)) {
      return ok(resolved);
    }
    return err(
      new FileError(
        "permission_denied",
        `path escapes confinement root ${this.rootPath}: ${input}`,
        input,
      ),
    );
  }

  async absolutePath(input: string, abortSignal?: AbortSignal): Promise<Result<string, FileError>> {
    const confined = this.confine(input);
    if (!confined.ok) return confined;
    return this.innerEnv.absolutePath(confined.value, abortSignal);
  }

  async joinPath(parts: string[], abortSignal?: AbortSignal): Promise<Result<string, FileError>> {
    const confined = this.confine(path.join(...parts));
    if (!confined.ok) return confined;
    return this.innerEnv.joinPath([confined.value], abortSignal);
  }

  async readTextFile(input: string, abortSignal?: AbortSignal): Promise<Result<string, FileError>> {
    const confined = this.confine(input);
    if (!confined.ok) return confined;
    return this.innerEnv.readTextFile(confined.value, abortSignal);
  }

  async readTextLines(
    input: string,
    options?: { maxLines?: number; abortSignal?: AbortSignal },
  ): Promise<Result<string[], FileError>> {
    const confined = this.confine(input);
    if (!confined.ok) return confined;
    return this.innerEnv.readTextLines(confined.value, options);
  }

  async readBinaryFile(
    input: string,
    abortSignal?: AbortSignal,
  ): Promise<Result<Uint8Array, FileError>> {
    const confined = this.confine(input);
    if (!confined.ok) return confined;
    return this.innerEnv.readBinaryFile(confined.value, abortSignal);
  }

  async writeFile(
    input: string,
    content: string | Uint8Array,
    abortSignal?: AbortSignal,
  ): Promise<Result<void, FileError>> {
    const confined = this.confine(input);
    if (!confined.ok) return confined;
    return this.innerEnv.writeFile(confined.value, content, abortSignal);
  }

  async appendFile(
    input: string,
    content: string | Uint8Array,
    abortSignal?: AbortSignal,
  ): Promise<Result<void, FileError>> {
    const confined = this.confine(input);
    if (!confined.ok) return confined;
    return this.innerEnv.appendFile(confined.value, content, abortSignal);
  }

  async fileInfo(input: string, abortSignal?: AbortSignal): Promise<Result<FileInfo, FileError>> {
    const confined = this.confine(input);
    if (!confined.ok) return confined;
    return this.innerEnv.fileInfo(confined.value, abortSignal);
  }

  async listDir(input: string, abortSignal?: AbortSignal): Promise<Result<FileInfo[], FileError>> {
    const confined = this.confine(input);
    if (!confined.ok) return confined;
    return this.innerEnv.listDir(confined.value, abortSignal);
  }

  // NOTE: the input path is confined syntactically, but the *resolved* path
  // may escape the root through a symlink. Callers needing symlink safety
  // must re-confine the returned canonical path (deferred hardening).
  async canonicalPath(
    input: string,
    abortSignal?: AbortSignal,
  ): Promise<Result<string, FileError>> {
    const confined = this.confine(input);
    if (!confined.ok) return confined;
    return this.innerEnv.canonicalPath(confined.value, abortSignal);
  }

  async exists(input: string, abortSignal?: AbortSignal): Promise<Result<boolean, FileError>> {
    const confined = this.confine(input);
    if (!confined.ok) return confined;
    return this.innerEnv.exists(confined.value, abortSignal);
  }

  async createDir(
    input: string,
    options?: { recursive?: boolean; abortSignal?: AbortSignal },
  ): Promise<Result<void, FileError>> {
    const confined = this.confine(input);
    if (!confined.ok) return confined;
    return this.innerEnv.createDir(confined.value, options);
  }

  async remove(
    input: string,
    options?: { recursive?: boolean; force?: boolean; abortSignal?: AbortSignal },
  ): Promise<Result<void, FileError>> {
    const confined = this.confine(input);
    if (!confined.ok) return confined;
    return this.innerEnv.remove(confined.value, options);
  }

  // Temp dirs/files land in the OS temp area by design; they are scratch
  // space, not user data, so they are not confined.
  async createTempDir(
    prefix?: string,
    abortSignal?: AbortSignal,
  ): Promise<Result<string, FileError>> {
    return this.innerEnv.createTempDir(prefix, abortSignal);
  }

  async createTempFile(options?: {
    prefix?: string;
    suffix?: string;
    abortSignal?: AbortSignal;
  }): Promise<Result<string, FileError>> {
    return this.innerEnv.createTempFile(options);
  }

  async exec(
    command: string,
    options?: ShellExecOptions,
  ): Promise<Result<{ stdout: string; stderr: string; exitCode: number }, ExecutionError>> {
    let cwd = options?.cwd;
    if (cwd !== undefined) {
      const confined = this.confine(cwd);
      if (!confined.ok) {
        return err(
          new ExecutionError(
            "spawn_error",
            `cwd escapes confinement root ${this.rootPath}: ${cwd}`,
          ),
        );
      }
      cwd = confined.value;
    }
    return this.innerEnv.exec(command, { ...options, cwd });
  }

  async cleanup(): Promise<void> {
    await this.innerEnv.cleanup();
  }
}
