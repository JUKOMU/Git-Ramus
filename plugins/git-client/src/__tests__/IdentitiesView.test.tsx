import "@testing-library/jest-dom/vitest";
import { act, cleanup, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { IdentityProfile } from "@git-ramus/contracts";
import type { GitClientApi } from "../api";
import { App } from "../App";

const profileId = "d23957ac-5c0f-4857-9124-7f1599a41f33";

const profile: IdentityProfile = {
  id: profileId,
  displayName: "Work profile",
  userName: "Alice Example",
  userEmail: "alice@example.com",
  gpgFormat: null,
  signingKey: null,
  signCommits: false,
  signTags: false,
  createdAt: "2026-07-19T00:00:00Z",
  updatedAt: "2026-07-19T00:00:00Z"
};

const personalProfile: IdentityProfile = {
  ...profile,
  id: "c8f98df3-e949-48e0-a9ad-407fe371a94a",
  displayName: "Personal profile",
  userEmail: "alice@personal.test"
};

afterEach(cleanup);

describe("IdentitiesView", () => {
  it("lets a fresh installation create its first profile and make it global", async () => {
    const user = userEvent.setup();
    const api = createApi({ identities: [], globalIdentityProfileId: null });
    vi.mocked(api.createIdentity).mockResolvedValue(profile);
    vi.mocked(api.setGlobalIdentity).mockResolvedValue(profile);

    render(<App api={api} route="/identities" />);

    expect(await screen.findByRole("heading", { name: "Identities" })).toBeInTheDocument();
    expect(screen.getByText("No identity profiles are configured.")).toBeInTheDocument();
    expect(screen.getByRole("checkbox", { name: "Set as global identity" })).toBeChecked();

    await user.type(screen.getByLabelText("Profile name"), profile.displayName);
    await user.type(screen.getByLabelText("Git user name"), profile.userName);
    await user.type(screen.getByLabelText("Git user email"), profile.userEmail);
    await user.click(screen.getByRole("button", { name: "Create identity" }));

    expect(api.createIdentity).toHaveBeenCalledWith({
      displayName: profile.displayName,
      userName: profile.userName,
      userEmail: profile.userEmail,
      gpgFormat: null,
      signingKey: null,
      signCommits: false,
      signTags: false
    });
    expect(api.setGlobalIdentity).toHaveBeenCalledWith({ profileId });
    expect(await screen.findByText("Global identity")).toBeInTheDocument();
  });

  it("creates a signed profile only after its format and key are configured", async () => {
    const user = userEvent.setup();
    const signedProfile: IdentityProfile = {
      ...profile,
      gpgFormat: "ssh",
      signingKey: "key::ssh-ed25519 AAAA-test",
      signCommits: true,
      signTags: true
    };
    const api = createApi({ identities: [], globalIdentityProfileId: null });
    vi.mocked(api.createIdentity).mockResolvedValue(signedProfile);
    render(<App api={api} route="/identities" />);

    await screen.findByText("No identity profiles are configured.");
    await user.type(screen.getByLabelText("Profile name"), signedProfile.displayName);
    await user.type(screen.getByLabelText("Git user name"), signedProfile.userName);
    await user.type(screen.getByLabelText("Git user email"), signedProfile.userEmail);
    await user.click(screen.getByRole("checkbox", { name: "Set as global identity" }));
    await user.click(screen.getByRole("checkbox", { name: "Sign commits" }));

    expect(screen.getByRole("button", { name: "Create identity" })).toBeDisabled();
    expect(
      screen.getByText("Choose a signing format and enter a signing key to enable signing.")
    ).toBeInTheDocument();

    await user.selectOptions(screen.getByLabelText("Signing format"), "ssh");
    expect(screen.getByRole("button", { name: "Create identity" })).toBeDisabled();
    const signingKey = screen.getByLabelText("Signing key");
    expect(signingKey).toHaveAttribute("type", "password");
    await user.type(signingKey, signedProfile.signingKey!);
    await user.click(screen.getByRole("checkbox", { name: "Sign tags" }));
    expect(screen.getByRole("button", { name: "Create identity" })).toBeEnabled();
    await user.click(screen.getByRole("button", { name: "Create identity" }));

    expect(api.createIdentity).toHaveBeenCalledWith({
      displayName: signedProfile.displayName,
      userName: signedProfile.userName,
      userEmail: signedProfile.userEmail,
      gpgFormat: "ssh",
      signingKey: signedProfile.signingKey,
      signCommits: true,
      signTags: true
    });
    expect(api.setGlobalIdentity).not.toHaveBeenCalled();
    expect(await screen.findByText("Commit signing enabled")).toBeInTheDocument();
    expect(screen.queryByText(signedProfile.signingKey!)).not.toBeInTheDocument();
  });

  it("submits a pending identity creation only once", async () => {
    const user = userEvent.setup();
    const created = deferred<IdentityProfile>();
    const api = createApi({ identities: [], globalIdentityProfileId: null });
    vi.mocked(api.createIdentity).mockImplementation(() => created.promise);
    vi.mocked(api.setGlobalIdentity).mockResolvedValue(profile);
    render(<App api={api} route="/identities" />);

    await screen.findByRole("heading", { name: "Identities" });
    await user.type(screen.getByLabelText("Profile name"), profile.displayName);
    await user.type(screen.getByLabelText("Git user name"), profile.userName);
    await user.type(screen.getByLabelText("Git user email"), profile.userEmail);
    await user.dblClick(screen.getByRole("button", { name: "Create identity" }));

    expect(api.createIdentity).toHaveBeenCalledOnce();
    expect(screen.getByRole("button", { name: "Creating identity…" })).toBeDisabled();

    await act(async () => {
      created.resolve(profile);
      await created.promise;
    });
    expect(await screen.findByText("Global identity")).toBeInTheDocument();
  });

  it("keeps a created profile recoverable when making it global fails", async () => {
    const user = userEvent.setup();
    const api = createApi({ identities: [], globalIdentityProfileId: null });
    vi.mocked(api.createIdentity).mockResolvedValue(profile);
    vi.mocked(api.setGlobalIdentity).mockRejectedValue(new Error("host unavailable"));
    render(<App api={api} route="/identities" />);

    await screen.findByText("No identity profiles are configured.");
    await user.type(screen.getByLabelText("Profile name"), profile.displayName);
    await user.type(screen.getByLabelText("Git user name"), profile.userName);
    await user.type(screen.getByLabelText("Git user email"), profile.userEmail);
    await user.click(screen.getByRole("button", { name: "Create identity" }));

    expect(await screen.findByRole("alert")).toHaveTextContent("Global identity could not be set.");
    expect(screen.getByRole("heading", { name: profile.displayName })).toBeInTheDocument();
    expect(screen.getByLabelText("Profile name")).toHaveValue("");
    expect(screen.getByLabelText("Git user name")).toHaveValue("");
    expect(screen.getByLabelText("Git user email")).toHaveValue("");
    expect(screen.getByRole("checkbox", { name: "Set as global identity" })).not.toBeChecked();
    expect(
      screen.getByRole("button", { name: "Set Work profile as global identity" })
    ).toBeEnabled();
  });

  it("ignores an older identity load after the API changes", async () => {
    const staleLoad = deferred<{
      identities: IdentityProfile[];
      globalIdentityProfileId: string | null;
    }>();
    const oldApi = createApi({ identities: [], globalIdentityProfileId: null });
    vi.mocked(oldApi.listIdentities).mockImplementation(() => staleLoad.promise);
    const currentProfile = personalProfile;
    const currentApi = createApi({ identities: [currentProfile], globalIdentityProfileId: null });
    const { rerender } = render(<App api={oldApi} route="/identities" />);

    rerender(<App api={currentApi} route="/identities" />);
    expect(await screen.findByText(currentProfile.displayName)).toBeInTheDocument();

    await act(async () => {
      staleLoad.resolve({ identities: [profile], globalIdentityProfileId: profile.id });
      await staleLoad.promise;
    });
    expect(screen.queryByText("Global identity")).not.toBeInTheDocument();
    expect(screen.getByText("Personal profile")).toBeInTheDocument();
    expect(screen.queryByText("Work profile")).not.toBeInTheDocument();
  });

  it("does not show profiles from the previous API while replacement data loads", async () => {
    const replacement = deferred<{
      identities: IdentityProfile[];
      globalIdentityProfileId: string | null;
    }>();
    const currentApi = createApi({
      identities: [personalProfile],
      globalIdentityProfileId: personalProfile.id
    });
    const replacementApi = createApi({ identities: [], globalIdentityProfileId: null });
    vi.mocked(replacementApi.listIdentities).mockImplementation(() => replacement.promise);
    const { rerender } = render(<App api={currentApi} route="/identities" />);
    expect(await screen.findByText("Personal profile")).toBeInTheDocument();

    rerender(<App api={replacementApi} route="/identities" />);
    expect(await screen.findByText("Loading identity profiles…")).toBeInTheDocument();
    expect(screen.queryByText("Personal profile")).not.toBeInTheDocument();
    expect(screen.queryByText("Global identity")).not.toBeInTheDocument();

    await act(async () => {
      replacement.resolve({ identities: [], globalIdentityProfileId: null });
      await replacement.promise;
    });
    expect(await screen.findByText("No identity profiles are configured.")).toBeInTheDocument();
  });

  it("loads and updates every signing field without rendering the key as text", async () => {
    const user = userEvent.setup();
    const signedProfile: IdentityProfile = {
      ...profile,
      gpgFormat: "ssh",
      signingKey: "C:/Users/Alice/.ssh/id_ed25519",
      signCommits: true
    };
    const updatedProfile = {
      ...signedProfile,
      displayName: "Primary work profile",
      userEmail: "alice@work.test",
      gpgFormat: "openpgp" as const,
      signCommits: false,
      signTags: true
    };
    const api = createApi({ identities: [signedProfile], globalIdentityProfileId: null });
    vi.mocked(api.updateIdentity).mockResolvedValue(updatedProfile);
    render(<App api={api} route="/identities" />);

    await user.click(await screen.findByRole("button", { name: "Edit Work profile" }));
    expect(screen.queryByText(signedProfile.signingKey!)).not.toBeInTheDocument();
    expect(screen.getByLabelText("Signing format")).toHaveValue("ssh");
    expect(screen.getByLabelText("Signing key")).toHaveAttribute("type", "password");
    expect(screen.getByLabelText("Signing key")).toHaveValue(signedProfile.signingKey);
    expect(screen.getByRole("checkbox", { name: "Sign commits" })).toBeChecked();
    expect(screen.getByRole("checkbox", { name: "Sign tags" })).not.toBeChecked();
    await user.clear(screen.getByLabelText("Profile name"));
    await user.type(screen.getByLabelText("Profile name"), updatedProfile.displayName);
    await user.clear(screen.getByLabelText("Git user email"));
    await user.type(screen.getByLabelText("Git user email"), updatedProfile.userEmail);
    await user.selectOptions(screen.getByLabelText("Signing format"), updatedProfile.gpgFormat);
    await user.click(screen.getByRole("checkbox", { name: "Sign commits" }));
    await user.click(screen.getByRole("checkbox", { name: "Sign tags" }));
    await user.click(screen.getByRole("button", { name: "Save identity" }));

    expect(api.updateIdentity).toHaveBeenCalledWith({
      profileId,
      displayName: updatedProfile.displayName,
      userName: signedProfile.userName,
      userEmail: updatedProfile.userEmail,
      gpgFormat: updatedProfile.gpgFormat,
      signingKey: signedProfile.signingKey,
      signCommits: updatedProfile.signCommits,
      signTags: updatedProfile.signTags
    });
    expect(await screen.findByText(updatedProfile.displayName)).toBeInTheDocument();
    expect(screen.queryByText(signedProfile.signingKey!)).not.toBeInTheDocument();
  });

  it("moves the single global marker and blocks deleting the current global profile", async () => {
    const user = userEvent.setup();
    const globalChange = deferred<IdentityProfile>();
    const deleted = deferred<void>();
    const api = createApi({
      identities: [profile, personalProfile],
      globalIdentityProfileId: profile.id
    });
    vi.mocked(api.setGlobalIdentity).mockImplementation(() => globalChange.promise);
    vi.mocked(api.deleteIdentity).mockImplementation(() => deleted.promise);
    render(<App api={api} route="/identities" />);

    const globalDelete = await screen.findByRole("button", { name: "Delete Work profile" });
    expect(globalDelete).toBeDisabled();
    expect(globalDelete).toHaveAttribute(
      "title",
      "Choose another global identity before deleting this profile"
    );
    expect(
      screen.getByText("Choose another global identity before deleting this profile.")
    ).toBeInTheDocument();

    await user.dblClick(
      screen.getByRole("button", { name: "Set Personal profile as global identity" })
    );
    expect(api.setGlobalIdentity).toHaveBeenCalledOnce();
    expect(api.setGlobalIdentity).toHaveBeenCalledWith({ profileId: personalProfile.id });
    expect(
      screen.getByRole("button", { name: "Setting Personal profile as global…" })
    ).toBeDisabled();

    await act(async () => {
      globalChange.resolve(personalProfile);
      await globalChange.promise;
    });
    expect(screen.getAllByText("Global identity")).toHaveLength(1);
    expect(screen.getByRole("button", { name: "Delete Personal profile" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "Delete Work profile" })).toBeEnabled();

    await user.dblClick(screen.getByRole("button", { name: "Delete Work profile" }));
    expect(api.deleteIdentity).toHaveBeenCalledOnce();
    expect(api.deleteIdentity).toHaveBeenCalledWith({ profileId });
    expect(screen.getByRole("button", { name: "Deleting Work profile…" })).toBeDisabled();

    await act(async () => {
      deleted.resolve();
      await deleted.promise;
    });
    expect(screen.queryByText("Work profile")).not.toBeInTheDocument();
    expect(screen.getByText("Personal profile")).toBeInTheDocument();
  });

  it("does not apply a completed create after the view switches to another API", async () => {
    const user = userEvent.setup();
    const created = deferred<IdentityProfile>();
    const oldApi = createApi({ identities: [], globalIdentityProfileId: null });
    vi.mocked(oldApi.createIdentity).mockImplementation(() => created.promise);
    const currentApi = createApi({
      identities: [personalProfile],
      globalIdentityProfileId: personalProfile.id
    });
    const { rerender } = render(<App api={oldApi} route="/identities" />);

    await screen.findByText("No identity profiles are configured.");
    await user.type(screen.getByLabelText("Profile name"), profile.displayName);
    await user.type(screen.getByLabelText("Git user name"), profile.userName);
    await user.type(screen.getByLabelText("Git user email"), profile.userEmail);
    await user.click(screen.getByRole("button", { name: "Create identity" }));
    rerender(<App api={currentApi} route="/identities" />);
    expect(await screen.findByText("Personal profile")).toBeInTheDocument();

    await act(async () => {
      created.resolve(profile);
      await created.promise;
    });
    expect(oldApi.setGlobalIdentity).not.toHaveBeenCalled();
    expect(screen.queryByText("Work profile")).not.toBeInTheDocument();
    expect(screen.getAllByText("Global identity")).toHaveLength(1);
  });

  it("does not continue a pending create after unmount", async () => {
    const user = userEvent.setup();
    const created = deferred<IdentityProfile>();
    const api = createApi({ identities: [], globalIdentityProfileId: null });
    vi.mocked(api.createIdentity).mockImplementation(() => created.promise);
    const { unmount } = render(<App api={api} route="/identities" />);

    await screen.findByText("No identity profiles are configured.");
    await user.type(screen.getByLabelText("Profile name"), profile.displayName);
    await user.type(screen.getByLabelText("Git user name"), profile.userName);
    await user.type(screen.getByLabelText("Git user email"), profile.userEmail);
    await user.click(screen.getByRole("button", { name: "Create identity" }));
    unmount();

    await act(async () => {
      created.resolve(profile);
      await created.promise;
    });
    expect(api.setGlobalIdentity).not.toHaveBeenCalled();
  });
});

function createApi(identityResult: {
  identities: IdentityProfile[];
  globalIdentityProfileId: string | null;
}): GitClientApi {
  return {
    listProjects: vi.fn(),
    createProject: vi.fn(),
    updateProjectScanRules: vi.fn(),
    scanProject: vi.fn(),
    listWorkspaces: vi.fn(),
    createWorkspace: vi.fn(),
    getWorkspaceMembership: vi.fn(),
    updateWorkspaceMembership: vi.fn(),
    deleteWorkspace: vi.fn(),
    getOverview: vi.fn(),
    getRepositorySnapshot: vi.fn(),
    getRepositoryChanges: vi.fn(),
    getRepositoryDiff: vi.fn(),
    getRepositoryTrustStatus: vi.fn(),
    stageRepository: vi.fn(),
    unstageRepository: vi.fn(),
    commitRepository: vi.fn(),
    trustRepository: vi.fn(),
    listIdentities: vi.fn(async () => identityResult),
    createIdentity: vi.fn(),
    updateIdentity: vi.fn(),
    deleteIdentity: vi.fn(),
    setGlobalIdentity: vi.fn(),
    bindRepositoryIdentity: vi.fn(),
    unbindRepositoryIdentity: vi.fn(),
    getEffectiveRepositoryIdentity: vi.fn()
  };
}

function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (reason?: unknown) => void;
  const promise = new Promise<T>((resolvePromise, rejectPromise) => {
    resolve = resolvePromise;
    reject = rejectPromise;
  });
  return { promise, resolve, reject };
}
