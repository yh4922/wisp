//! 「打开远程」对话框：选择主机 → 新建 / 编辑主机（可测试连接）→ 浏览远程目录 → 命名并打开。

use std::path::PathBuf;

use gpui::{
    ClickEvent, DismissEvent, Entity, EventEmitter, FocusHandle, Focusable, PathPromptOptions,
    PromptLevel, ScrollHandle, Subscription, Task, TaskExt as _, WeakEntity,
};
use menu::{Cancel, Confirm, SelectNext, SelectPrevious};
use ui::{Checkbox, ListItem, ListItemSpacing, TintColor, ToggleState, prelude::*};
use ui_input::InputField;
use util::ResultExt as _;
use wisp_workspaces::{
    AuthMethod, DEFAULT_SSH_PORT, Host, HostId, WorkspaceLocation, WorkspaceStore,
    delete_host_secret, load_host_secret, save_host_secret,
};
use workspace::{ModalView, Workspace};

use super::ssh_command::{self, RemoteDirListing, SshTarget};

/// 主机列表行的地址：`user@host:port`。
fn host_address(host: &Host) -> String {
    if host.username.is_empty() {
        format!("{}:{}", host.host, host.port)
    } else {
        format!("{}@{}:{}", host.username, host.host, host.port)
    }
}

fn new_field(
    window: &mut Window,
    cx: &mut App,
    label: &'static str,
    placeholder: &'static str,
    masked: bool,
) -> Entity<InputField> {
    cx.new(|cx| {
        // 默认最小宽度 192px 会把窄列（端口）撑出对话框，宽度交给外层布局决定。
        let field = InputField::new(window, cx, placeholder)
            .label(label)
            .label_min_width(px(0.));
        if masked { field.masked(true) } else { field }
    })
}

fn field_text(field: &Entity<InputField>, cx: &App) -> String {
    field.read(cx).text(cx).trim().to_string()
}

fn set_field_text(field: &Entity<InputField>, text: &str, window: &mut Window, cx: &mut App) {
    field.update(cx, |field, cx| field.set_text(text, window, cx));
}

fn focus_field(field: &Entity<InputField>, window: &mut Window, cx: &mut App) {
    let handle = field.read(cx).focus_handle(cx);
    window.focus(&handle, cx);
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum TestState {
    Idle,
    Running,
    Succeeded,
    Failed(SharedString),
}

/// 新建 / 编辑主机的表单。
struct HostForm {
    /// 编辑时为原主机信息。
    editing: Option<Host>,
    has_saved_secret: bool,
    name: Entity<InputField>,
    host: Entity<InputField>,
    port: Entity<InputField>,
    username: Entity<InputField>,
    password: Entity<InputField>,
    key_path: Entity<InputField>,
    passphrase: Entity<InputField>,
    auth: AuthMethod,
    error: Option<SharedString>,
    test: TestState,
    _test_task: Option<Task<()>>,
}

impl HostForm {
    fn new(editing: Option<Host>, window: &mut Window, cx: &mut App) -> Self {
        let has_saved_secret = editing.is_some();
        let password_placeholder = if has_saved_secret {
            "Leave empty to keep the saved password"
        } else {
            "Password"
        };
        let form = Self {
            name: new_field(window, cx, i18n::t("Display Name"), i18n::t("e.g. Test Server"), false),
            host: new_field(window, cx, i18n::t("Host"), i18n::t("IP address or domain name"), false),
            port: new_field(window, cx, i18n::t("Port"), "22", false),
            username: new_field(window, cx, i18n::t("Username"), i18n::t("e.g. root"), false),
            password: new_field(
                window,
                cx,
                i18n::t("Password"),
                i18n::t(password_placeholder),
                true,
            ),
            key_path: new_field(
                window,
                cx,
                i18n::t("Private Key"),
                i18n::t("Path to the private key file"),
                false,
            ),
            passphrase: new_field(
                window,
                cx,
                i18n::t("Passphrase (optional)"),
                if has_saved_secret {
                    i18n::t("Leave empty to keep the saved passphrase")
                } else {
                    i18n::t("Private key passphrase")
                },
                true,
            ),
            auth: editing.as_ref().map(|host| host.auth).unwrap_or_default(),
            editing,
            has_saved_secret,
            error: None,
            test: TestState::Idle,
            _test_task: None,
        };
        let port = form
            .editing
            .as_ref()
            .map(|host| host.port)
            .unwrap_or(DEFAULT_SSH_PORT);
        set_field_text(&form.port, &port.to_string(), window, cx);
        if let Some(host) = form.editing.clone() {
            set_field_text(&form.name, &host.name, window, cx);
            set_field_text(&form.host, &host.host, window, cx);
            set_field_text(&form.username, &host.username, window, cx);
            if let Some(key_path) = &host.key_path {
                set_field_text(&form.key_path, &key_path.to_string_lossy(), window, cx);
            }
        }
        form
    }

    /// 校验表单，返回主机信息（id 沿用编辑中的主机）以及输入的密码 / 口令（为空时 `None`）。
    fn validate(&mut self, cx: &App) -> Option<(Host, Option<String>)> {
        self.error = None;
        let host_name = field_text(&self.host, cx);
        let port_text = field_text(&self.port, cx);
        let username = field_text(&self.username, cx);
        let key_path = field_text(&self.key_path, cx);
        let error = if host_name.is_empty() || host_name.contains(char::is_whitespace) {
            Some(i18n::t("Please enter a valid host (IP address or domain name)."))
        } else if !matches!(port_text.parse::<u16>(), Ok(port) if port >= 1) {
            Some(i18n::t("Port must be a number between 1 and 65535."))
        } else if username.is_empty() {
            Some(i18n::t("Please enter a username."))
        } else if self.auth == AuthMethod::Key && key_path.is_empty() {
            Some(i18n::t("Please choose a private key file."))
        } else {
            None
        };
        if let Some(error) = error {
            self.error = Some(error.into());
            return None;
        }
        let port = port_text.parse::<u16>().unwrap_or(DEFAULT_SSH_PORT);
        let mut host = Host::new(
            field_text(&self.name, cx),
            host_name,
            port,
            username,
            self.auth,
            (self.auth == AuthMethod::Key).then(|| PathBuf::from(key_path)),
        );
        if let Some(editing) = &self.editing {
            host.id = editing.id;
        }
        let secret_field = match self.auth {
            AuthMethod::Password => &self.password,
            AuthMethod::Key => &self.passphrase,
        };
        // 密码可能有意包含首尾空格，不做 trim。
        let secret = secret_field.read(cx).text(cx);
        let secret = (!secret.is_empty()).then_some(secret);
        Some((host, secret))
    }

    /// 已保存的凭据是否仍适用于当前表单（编辑中、且认证方式没有改变）。
    fn saved_secret_applies(&self) -> Option<HostId> {
        let editing = self.editing.as_ref()?;
        (self.has_saved_secret && editing.auth == self.auth).then_some(editing.id)
    }
}

/// 远程目录浏览。
struct Browser {
    host: Host,
    /// 凭据；`secret_loaded` 为 false 时尚未从凭据管理器读取。
    secret: Option<String>,
    secret_loaded: bool,
    path_input: Entity<InputField>,
    listing: Option<RemoteDirListing>,
    loading: bool,
    error: Option<SharedString>,
    selected: Option<usize>,
    scroll_handle: ScrollHandle,
    _task: Option<Task<()>>,
}

impl Browser {
    /// 列表项：非根目录时第一项为 `..`。
    fn entries(&self) -> Vec<String> {
        let Some(listing) = &self.listing else {
            return Vec::new();
        };
        let mut entries = Vec::with_capacity(listing.dirs.len() + 1);
        if listing.path != "/" {
            entries.push("..".to_string());
        }
        entries.extend(listing.dirs.iter().cloned());
        entries
    }
}

/// 命名并打开。
struct NameStep {
    host: Host,
    path: String,
    name: Entity<InputField>,
    save: bool,
}

enum Step {
    Hosts,
    Form(HostForm),
    Browse(Browser),
    Name(NameStep),
}

/// 「打开远程」对话框。
pub struct RemoteDialog {
    workspace: WeakEntity<Workspace>,
    store: Entity<WorkspaceStore>,
    focus_handle: FocusHandle,
    step: Step,
    _store_subscription: Subscription,
}

impl EventEmitter<DismissEvent> for RemoteDialog {}

impl ModalView for RemoteDialog {}

impl Focusable for RemoteDialog {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl RemoteDialog {
    pub fn new(workspace: WeakEntity<Workspace>, _window: &mut Window, cx: &mut Context<Self>) -> Self {
        let store = WorkspaceStore::get_or_init(cx);
        let subscription = cx.observe(&store, |_, _, cx| cx.notify());
        Self {
            workspace,
            store,
            focus_handle: cx.focus_handle(),
            step: Step::Hosts,
            _store_subscription: subscription,
        }
    }

    fn dismiss(&mut self, _: &Cancel, _: &mut Window, cx: &mut Context<Self>) {
        cx.emit(DismissEvent);
    }

    fn confirm(&mut self, _: &Confirm, window: &mut Window, cx: &mut Context<Self>) {
        match &self.step {
            Step::Hosts => {}
            Step::Form(_) => self.save_form(window, cx),
            Step::Browse(_) => self.browse_confirm(window, cx),
            Step::Name(_) => self.open(window, cx),
        }
    }

    fn select_next(&mut self, _: &SelectNext, _: &mut Window, cx: &mut Context<Self>) {
        self.move_selection(1, cx);
    }

    fn select_previous(&mut self, _: &SelectPrevious, _: &mut Window, cx: &mut Context<Self>) {
        self.move_selection(-1, cx);
    }

    fn move_selection(&mut self, delta: isize, cx: &mut Context<Self>) {
        let Step::Browse(browser) = &mut self.step else {
            return;
        };
        let count = browser.entries().len();
        if count == 0 {
            return;
        }
        let next = match browser.selected {
            None => 0,
            Some(index) => (index as isize + delta).clamp(0, count as isize - 1) as usize,
        };
        browser.selected = Some(next);
        browser.scroll_handle.scroll_to_item(next);
        cx.notify();
    }

    // ---- 步骤切换 ----

    fn show_hosts(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.step = Step::Hosts;
        window.focus(&self.focus_handle, cx);
        cx.notify();
    }

    fn show_form(&mut self, editing: Option<Host>, window: &mut Window, cx: &mut Context<Self>) {
        let form = HostForm::new(editing, window, cx);
        focus_field(&form.host, window, cx);
        self.step = Step::Form(form);
        cx.notify();
    }

    fn show_browser(
        &mut self,
        host: Host,
        secret: Option<Option<String>>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let path_input = new_field(window, cx, i18n::t("Remote Folder"), i18n::t("Remote path, e.g. /srv/app"), false);
        set_field_text(&path_input, "~", window, cx);
        focus_field(&path_input, window, cx);
        self.step = Step::Browse(Browser {
            host,
            secret_loaded: secret.is_some(),
            secret: secret.flatten(),
            path_input,
            listing: None,
            loading: false,
            error: None,
            selected: None,
            scroll_handle: ScrollHandle::new(),
            _task: None,
        });
        self.navigate("~".to_string(), window, cx);
    }

    fn show_name_step(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Step::Browse(browser) = &self.step else {
            return;
        };
        let Some(listing) = &browser.listing else {
            return;
        };
        let host = browser.host.clone();
        let path = listing.path.clone();
        let name = new_field(window, cx, i18n::t("Workspace Name"), i18n::t("Workspace name"), false);
        set_field_text(&name, &ssh_command::remote_dir_name(&path), window, cx);
        focus_field(&name, window, cx);
        self.step = Step::Name(NameStep {
            host,
            path,
            name,
            save: true,
        });
        cx.notify();
    }

    // ---- 主机列表 ----

    fn delete_host(&mut self, host: Host, window: &mut Window, cx: &mut Context<Self>) {
        let message = i18n::tf!("Delete host \"{name}\"?", name = host.display_name());
        let answer = window.prompt(
            PromptLevel::Warning,
            &message,
            Some(i18n::t(
                "Its saved password and the remote workspaces on this host will also be removed.",
            )),
            &[i18n::t("Delete"), i18n::t("Cancel")],
            cx,
        );
        let store = self.store.clone();
        cx.spawn(async move |_, cx| {
            if answer.await.ok() != Some(0) {
                return;
            }
            cx.update(|cx| {
                store.update(cx, |store, cx| store.remove_host(host.id, cx));
                delete_host_secret(host.id, cx).detach_and_log_err(cx);
            });
        })
        .detach();
    }

    // ---- 主机表单 ----

    fn set_auth(&mut self, auth: AuthMethod, cx: &mut Context<Self>) {
        if let Step::Form(form) = &mut self.step {
            form.auth = auth;
            form.test = TestState::Idle;
            cx.notify();
        }
    }

    fn browse_key_file(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let paths = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: None,
        });
        cx.spawn_in(window, async move |this, cx| {
            let Some(path) = paths
                .await
                .ok()
                .and_then(|result| result.log_err())
                .flatten()
                .and_then(|paths| paths.into_iter().next())
            else {
                return;
            };
            this.update_in(cx, |this, window, cx| {
                if let Step::Form(form) = &this.step {
                    set_field_text(&form.key_path, &path.to_string_lossy(), window, cx);
                }
            })
            .ok();
        })
        .detach();
    }

    fn test_connection(&mut self, cx: &mut Context<Self>) {
        let Step::Form(form) = &mut self.step else {
            return;
        };
        let Some((host, secret)) = form.validate(cx) else {
            cx.notify();
            return;
        };
        let saved_secret_id = if secret.is_none() {
            form.saved_secret_applies()
        } else {
            None
        };
        form.test = TestState::Running;
        form._test_task = Some(cx.spawn(async move |this, cx| {
            let secret = match (secret, saved_secret_id) {
                (Some(secret), _) => Some(secret),
                (None, Some(id)) => cx
                    .update(|cx| load_host_secret(id, cx))
                    .await
                    .log_err()
                    .flatten(),
                (None, None) => None,
            };
            let result =
                ssh_command::test_connection(SshTarget::from_host(&host, secret), cx).await;
            this.update(cx, |this, cx| {
                if let Step::Form(form) = &mut this.step {
                    form.test = match result {
                        Ok(()) => TestState::Succeeded,
                        Err(error) => TestState::Failed(format!("{error:#}").into()),
                    };
                    cx.notify();
                }
            })
            .ok();
        }));
        cx.notify();
    }

    fn save_form(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Step::Form(form) = &mut self.step else {
            return;
        };
        let Some((host, secret)) = form.validate(cx) else {
            cx.notify();
            return;
        };
        let is_new = form.editing.is_none();
        let auth_changed = form
            .editing
            .as_ref()
            .is_some_and(|editing| editing.auth != host.auth);
        let host_id = host.id;
        self.store.update(cx, |store, cx| {
            if is_new {
                store.add_host(host.clone(), cx);
            } else {
                store.update_host(host.clone(), cx);
            }
        });
        // 密码 / 口令只进系统凭据管理器。编辑时留空表示不修改；但认证方式变了时旧凭据已不适用。
        if let Some(secret) = secret.clone() {
            save_host_secret(host_id, secret, cx).detach_and_log_err(cx);
        } else if auth_changed {
            delete_host_secret(host_id, cx).detach_and_log_err(cx);
        }
        if is_new {
            self.show_browser(host, Some(secret), window, cx);
        } else {
            self.show_hosts(window, cx);
        }
    }

    // ---- 目录浏览 ----

    fn navigate(&mut self, path: String, window: &mut Window, cx: &mut Context<Self>) {
        let Step::Browse(browser) = &mut self.step else {
            return;
        };
        browser.loading = true;
        browser.error = None;
        let host = browser.host.clone();
        let known_secret = browser.secret_loaded.then(|| browser.secret.clone());
        browser._task = Some(cx.spawn_in(window, async move |this, cx| {
            let secret = match known_secret {
                Some(secret) => secret,
                None => {
                    let secret = cx
                        .update(|_, cx| load_host_secret(host.id, cx))
                        .ok();
                    let secret = match secret {
                        Some(task) => task.await.log_err().flatten(),
                        None => None,
                    };
                    this.update(cx, |this, _| {
                        if let Step::Browse(browser) = &mut this.step {
                            browser.secret = secret.clone();
                            browser.secret_loaded = true;
                        }
                    })
                    .ok();
                    secret
                }
            };
            let result =
                ssh_command::list_dirs(SshTarget::from_host(&host, secret), path, cx).await;
            this.update_in(cx, |this, window, cx| {
                let Step::Browse(browser) = &mut this.step else {
                    return;
                };
                browser.loading = false;
                match result {
                    Ok(listing) => {
                        set_field_text(&browser.path_input, &listing.path, window, cx);
                        browser.listing = Some(listing);
                        browser.selected = None;
                        browser.scroll_handle.scroll_to_item(0);
                    }
                    Err(error) => {
                        browser.error = Some(format!("{error:#}").into());
                    }
                }
                cx.notify();
            })
            .ok();
        }));
        cx.notify();
    }

    fn enter_entry(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Step::Browse(browser) = &self.step else {
            return;
        };
        let Some(listing) = &browser.listing else {
            return;
        };
        let Some(entry) = browser.entries().get(index).cloned() else {
            return;
        };
        let path = ssh_command::join_remote_path(&listing.path, &entry);
        self.navigate(path, window, cx);
    }

    /// 回车：路径框内容变化时跳转到输入的路径，否则进入选中的子目录。
    fn browse_confirm(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Step::Browse(browser) = &self.step else {
            return;
        };
        if browser.loading {
            return;
        }
        let typed = field_text(&browser.path_input, cx);
        let current = browser.listing.as_ref().map(|listing| listing.path.clone());
        if !typed.is_empty() && Some(&typed) != current.as_ref() {
            self.navigate(typed, window, cx);
        } else if let Some(index) = browser.selected {
            self.enter_entry(index, window, cx);
        }
    }

    // ---- 命名并打开 ----

    fn open(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Step::Name(step) = &self.step else {
            return;
        };
        let mut name = field_text(&step.name, cx);
        if name.is_empty() {
            name = ssh_command::remote_dir_name(&step.path);
        }
        let host = step.host.clone();
        let path = step.path.clone();
        let save = step.save;
        let workspace = self.workspace.clone();
        cx.emit(DismissEvent);
        if save {
            let id = self.store.update(cx, |store, cx| {
                match store.find_remote_workspace(host.id, &path).map(|saved| saved.id) {
                    Some(id) => {
                        store.rename_workspace(id, name, cx);
                        id
                    }
                    None => store.add_workspace(
                        name,
                        WorkspaceLocation::Remote {
                            host_id: host.id,
                            path: path.clone(),
                        },
                        cx,
                    ),
                }
            });
            super::open_remote_workspace(workspace, id, Some(host), path, false, window, cx);
        } else {
            super::open_host_path(workspace, host, path, None, false, window, cx);
        }
    }

    // ---- 渲染 ----

    fn render_header(&self, title: &'static str) -> impl IntoElement {
        h_flex()
            .px_3()
            .pt_2()
            .pb_1()
            .w_full()
            .gap_1p5()
            .child(Icon::new(IconName::Server).size(IconSize::XSmall))
            .child(Headline::new(title).size(HeadlineSize::XSmall))
    }

    fn render_hosts(&self, cx: &mut Context<Self>) -> AnyElement {
        let hosts: Vec<Host> = self.store.read(cx).hosts().to_vec();
        let rows: Vec<AnyElement> = hosts
            .into_iter()
            .enumerate()
            .map(|(ix, host)| {
                let open_host = host.clone();
                let edit_host = host.clone();
                let delete_host = host.clone();
                ListItem::new(("remote-host", ix))
                    .inset(true)
                    .spacing(ListItemSpacing::Sparse)
                    .start_slot(Icon::new(IconName::Server).color(Color::Muted).size(IconSize::Small))
                    .child(
                        v_flex()
                            .min_w_0()
                            .w_full()
                            .child(Label::new(host.display_name()).truncate())
                            .child(
                                Label::new(host_address(&host))
                                    .size(LabelSize::XSmall)
                                    .color(Color::Muted)
                                    .truncate(),
                            ),
                    )
                    .end_slot(
                        h_flex()
                            .gap_0p5()
                            .child(
                                IconButton::new(("edit-host", ix), IconName::Pencil)
                                    .icon_size(IconSize::Small)
                                    .icon_color(Color::Muted)
                                    .tooltip(ui::Tooltip::text(i18n::t("Edit Host")))
                                    .on_click(cx.listener(move |this, _, window, cx| {
                                        cx.stop_propagation();
                                        this.show_form(Some(edit_host.clone()), window, cx);
                                    })),
                            )
                            .child(
                                IconButton::new(("delete-host", ix), IconName::Trash)
                                    .icon_size(IconSize::Small)
                                    .icon_color(Color::Muted)
                                    .tooltip(ui::Tooltip::text(i18n::t("Delete Host")))
                                    .on_click(cx.listener(move |this, _, window, cx| {
                                        cx.stop_propagation();
                                        this.delete_host(delete_host.clone(), window, cx);
                                    })),
                            ),
                    )
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.show_browser(open_host.clone(), None, window, cx);
                    }))
                    .into_any_element()
            })
            .collect();

        v_flex()
            .w_full()
            .child(self.render_header(i18n::t("Open Remote")))
            .child(
                div()
                    .px_3()
                    .pb_1()
                    .child(
                        Label::new(i18n::t("Choose a host to connect to"))
                            .size(LabelSize::Small)
                            .color(Color::Muted),
                    ),
            )
            .child(
                v_flex()
                    .id("remote-host-list")
                    .w_full()
                    .max_h(rems(20.))
                    .overflow_y_scroll()
                    .py_1()
                    .when(rows.is_empty(), |this| {
                        this.child(
                            div().px_3().py_2().child(
                                Label::new(i18n::t(
                                    "No saved hosts yet. Click \"New Host\" to add one.",
                                ))
                                .size(LabelSize::Small)
                                .color(Color::Muted),
                            ),
                        )
                    })
                    .children(rows),
            )
            .child(
                h_flex()
                    .px_3()
                    .pb_3()
                    .pt_1()
                    .w_full()
                    .justify_between()
                    .child(
                        Button::new("remote-new-host", i18n::t("New Host"))
                            .style(ButtonStyle::Filled)
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.show_form(None, window, cx)
                            })),
                    )
                    .child(
                        Button::new("remote-hosts-cancel", i18n::t("Cancel")).on_click(
                            cx.listener(|this, _, window, cx| this.dismiss(&Cancel, window, cx)),
                        ),
                    ),
            )
            .into_any_element()
    }

    fn render_form(&self, form: &HostForm, cx: &mut Context<Self>) -> AnyElement {
        let title = if form.editing.is_some() {
            i18n::t("Edit Host")
        } else {
            i18n::t("New Host")
        };
        let auth = form.auth;
        let auth_button = |id: &'static str, label: &'static str, value: AuthMethod| {
            Button::new(id, label)
                .style(ButtonStyle::Subtle)
                .selected_style(ButtonStyle::Tinted(TintColor::Accent))
                .toggle_state(auth == value)
                .on_click(cx.listener(move |this, _, _, cx| this.set_auth(value, cx)))
        };
        let test_status: Option<AnyElement> = match &form.test {
            TestState::Idle => None,
            TestState::Running => Some(
                h_flex()
                    .gap_1()
                    .child(
                        Icon::new(IconName::LoadCircle)
                            .size(IconSize::Small)
                            .color(Color::Muted),
                    )
                    .child(
                        Label::new(i18n::t("Testing connection…"))
                            .size(LabelSize::Small)
                            .color(Color::Muted),
                    )
                    .into_any_element(),
            ),
            TestState::Succeeded => Some(
                h_flex()
                    .gap_1()
                    .child(Icon::new(IconName::Check).size(IconSize::Small).color(Color::Success))
                    .child(
                        Label::new(i18n::t("Connection succeeded"))
                            .size(LabelSize::Small)
                            .color(Color::Success),
                    )
                    .into_any_element(),
            ),
            TestState::Failed(error) => Some(
                h_flex()
                    .w_full()
                    .items_start()
                    .gap_1()
                    .child(Icon::new(IconName::XCircle).size(IconSize::Small).color(Color::Error))
                    .child(
                        v_flex()
                            .min_w_0()
                            .flex_1()
                            .child(
                                Label::new(i18n::t("Connection failed"))
                                    .size(LabelSize::Small)
                                    .color(Color::Error),
                            )
                            .child(
                                Label::new(error.clone())
                                    .size(LabelSize::XSmall)
                                    .color(Color::Muted),
                            ),
                    )
                    .into_any_element(),
            ),
        };
        let testing = form.test == TestState::Running;

        v_flex()
            .w_full()
            .child(self.render_header(title))
            .child(
                v_flex()
                    .px_3()
                    .pb_3()
                    .w_full()
                    .gap_2()
                    .child(form.name.clone())
                    .child(
                        h_flex()
                            .w_full()
                            .gap_2()
                            .items_start()
                            .child(div().flex_1().child(form.host.clone()))
                            .child(div().w(rems(7.)).child(form.port.clone())),
                    )
                    .child(form.username.clone())
                    .child(
                        v_flex()
                            .gap_1()
                            .child(Label::new(i18n::t("Authentication")).size(LabelSize::Small))
                            .child(
                                h_flex()
                                    .gap_1()
                                    .child(auth_button(
                                        "remote-auth-password",
                                        i18n::t("Password"),
                                        AuthMethod::Password,
                                    ))
                                    .child(auth_button(
                                        "remote-auth-key",
                                        i18n::t("Private Key"),
                                        AuthMethod::Key,
                                    )),
                            ),
                    )
                    .when(auth == AuthMethod::Password, |this| {
                        this.child(form.password.clone())
                    })
                    .when(auth == AuthMethod::Key, |this| {
                        this.child(
                            h_flex()
                                .w_full()
                                .gap_2()
                                .items_end()
                                .child(div().flex_1().child(form.key_path.clone()))
                                .child(
                                    Button::new("remote-browse-key", i18n::t("Browse…"))
                                        .style(ButtonStyle::Outlined)
                                        .on_click(cx.listener(|this, _, window, cx| {
                                            this.browse_key_file(window, cx)
                                        })),
                                ),
                        )
                        .child(form.passphrase.clone())
                    })
                    .when_some(form.error.clone(), |this, error| {
                        this.child(Label::new(error).size(LabelSize::Small).color(Color::Error))
                    })
                    .children(test_status)
                    .child(
                        h_flex()
                            .w_full()
                            .pt_1()
                            .justify_between()
                            .child(
                                Button::new("remote-test-connection", i18n::t("Test Connection"))
                                    .style(ButtonStyle::Outlined)
                                    .disabled(testing)
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.test_connection(cx)
                                    })),
                            )
                            .child(
                                h_flex()
                                    .gap_1()
                                    .child(
                                        Button::new("remote-form-cancel", i18n::t("Cancel"))
                                            .on_click(cx.listener(|this, _, window, cx| {
                                                this.show_hosts(window, cx)
                                            })),
                                    )
                                    .child(
                                        Button::new("remote-form-save", i18n::t("Save"))
                                            .style(ButtonStyle::Filled)
                                            .on_click(cx.listener(|this, _, window, cx| {
                                                this.save_form(window, cx)
                                            })),
                                    ),
                            ),
                    ),
            )
            .into_any_element()
    }

    fn render_browser(&self, browser: &Browser, cx: &mut Context<Self>) -> AnyElement {
        let entries = browser.entries();
        let selected = browser.selected;
        let rows: Vec<AnyElement> = entries
            .into_iter()
            .enumerate()
            .map(|(ix, entry)| {
                let icon = if entry == ".." {
                    IconName::ArrowLeft
                } else {
                    IconName::Folder
                };
                ListItem::new(("remote-dir", ix))
                    .inset(true)
                    .spacing(ListItemSpacing::Dense)
                    .toggle_state(selected == Some(ix))
                    .start_slot(Icon::new(icon).color(Color::Muted).size(IconSize::Small))
                    .child(Label::new(entry).truncate())
                    .on_click(cx.listener(move |this, event: &ClickEvent, window, cx| {
                        if event.click_count() >= 2 {
                            this.enter_entry(ix, window, cx);
                        } else if let Step::Browse(browser) = &mut this.step {
                            browser.selected = Some(ix);
                            cx.notify();
                        }
                    }))
                    .into_any_element()
            })
            .collect();
        let can_select = browser.listing.is_some() && !browser.loading;

        v_flex()
            .w_full()
            .child(self.render_header(i18n::t("Choose Remote Folder")))
            .child(
                div().px_3().pb_1().child(
                    Label::new(host_address(&browser.host))
                        .size(LabelSize::Small)
                        .color(Color::Muted),
                ),
            )
            .child(div().px_3().child(browser.path_input.clone()))
            .child(
                v_flex()
                    .id("remote-dir-list")
                    .track_scroll(&browser.scroll_handle)
                    .mx_3()
                    .my_2()
                    .h(rems(16.))
                    .overflow_y_scroll()
                    .border_1()
                    .rounded_md()
                    .border_color(cx.theme().colors().border_variant)
                    .py_1()
                    .when(browser.loading, |this| {
                        this.child(
                            h_flex()
                                .px_2()
                                .gap_1()
                                .child(
                                    Icon::new(IconName::LoadCircle)
                                        .size(IconSize::Small)
                                        .color(Color::Muted),
                                )
                                .child(
                                    Label::new(i18n::t("Connecting and loading folders…"))
                                        .size(LabelSize::Small)
                                        .color(Color::Muted),
                                ),
                        )
                    })
                    .when_some(browser.error.clone(), |this, error| {
                        this.child(
                            v_flex()
                                .px_2()
                                .child(
                                    Label::new(i18n::t("Failed to list the remote folder"))
                                        .size(LabelSize::Small)
                                        .color(Color::Error),
                                )
                                .child(
                                    Label::new(error)
                                        .size(LabelSize::XSmall)
                                        .color(Color::Muted),
                                ),
                        )
                    })
                    .when(
                        !browser.loading
                            && browser.error.is_none()
                            && browser.listing.as_ref().is_some_and(|l| l.dirs.is_empty()),
                        |this| {
                            this.child(
                                div().px_2().child(
                                    Label::new(i18n::t("No subfolders"))
                                        .size(LabelSize::Small)
                                        .color(Color::Muted),
                                ),
                            )
                        },
                    )
                    .children(rows),
            )
            .child(
                h_flex()
                    .px_3()
                    .pb_3()
                    .w_full()
                    .justify_between()
                    .child(
                        Button::new("remote-browse-back", i18n::t("Back")).on_click(cx.listener(
                            |this, _, window, cx| this.show_hosts(window, cx),
                        )),
                    )
                    .child(
                        Button::new("remote-browse-select", i18n::t("Select This Folder"))
                            .style(ButtonStyle::Filled)
                            .disabled(!can_select)
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.show_name_step(window, cx)
                            })),
                    ),
            )
            .into_any_element()
    }

    fn render_name(&self, step: &NameStep, cx: &mut Context<Self>) -> AnyElement {
        let save = step.save;
        v_flex()
            .w_full()
            .child(self.render_header(i18n::t("Open Remote Workspace")))
            .child(
                v_flex()
                    .px_3()
                    .pb_3()
                    .w_full()
                    .gap_2()
                    .child(
                        Label::new(format!("{}:{}", host_address(&step.host), step.path))
                            .size(LabelSize::Small)
                            .color(Color::Muted)
                            .truncate(),
                    )
                    .child(step.name.clone())
                    .child(
                        Checkbox::new(
                            "remote-save-workspace",
                            if save {
                                ToggleState::Selected
                            } else {
                                ToggleState::Unselected
                            },
                        )
                        .label(i18n::t("Save as Workspace"))
                        .on_click(cx.listener(|this, state: &ToggleState, _, cx| {
                            if let Step::Name(step) = &mut this.step {
                                step.save = *state == ToggleState::Selected;
                                cx.notify();
                            }
                        })),
                    )
                    .child(
                        h_flex()
                            .w_full()
                            .pt_1()
                            .justify_between()
                            .child(Button::new("remote-name-back", i18n::t("Back")).on_click(
                                cx.listener(|this, _, window, cx| {
                                    let Step::Name(step) = &this.step else {
                                        return;
                                    };
                                    let host = step.host.clone();
                                    let path = step.path.clone();
                                    this.show_browser(host, None, window, cx);
                                    this.navigate(path, window, cx);
                                }),
                            ))
                            .child(
                                Button::new("remote-name-open", i18n::t("Open"))
                                    .style(ButtonStyle::Filled)
                                    .on_click(
                                        cx.listener(|this, _, window, cx| this.open(window, cx)),
                                    ),
                            ),
                    ),
            )
            .into_any_element()
    }
}

impl Render for RemoteDialog {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let content = match &self.step {
            Step::Hosts => self.render_hosts(cx),
            Step::Form(form) => self.render_form(form, cx),
            Step::Browse(browser) => self.render_browser(browser, cx),
            Step::Name(step) => self.render_name(step, cx),
        };
        v_flex()
            .key_context("RemoteDialog")
            .track_focus(&self.focus_handle)
            .on_action(cx.listener(Self::dismiss))
            .on_action(cx.listener(Self::confirm))
            .on_action(cx.listener(Self::select_next))
            .on_action(cx.listener(Self::select_previous))
            .elevation_2(cx)
            .w(rems(34.))
            .child(content)
    }
}
