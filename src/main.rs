use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEventKind},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, ListState, Paragraph, Wrap},
    Frame, Terminal,
};
use std::io::{self, stdout};
use std::time::{Duration, Instant};
use sysinfo::{Pid, System};

#[derive(PartialEq)]
enum AppMode {
    ProcessSelect,
    TimerInput,
    TimerRunning,
}

struct App {
    system: System,
    processes: Vec<(Pid, String)>,
    filtered_processes: Vec<(Pid, String)>,
    list_state: ListState,
    search_query: String,
    selected_pid: Option<Pid>,
    mode: AppMode,
    timer_input: String,
    timer_seconds: u64,
    timer_start: Option<Instant>,
    status_message: String,
}

impl App {
    fn new() -> Self {
        let mut app = App {
            system: System::new(),
            processes: Vec::new(),
            filtered_processes: Vec::new(),
            list_state: ListState::default(),
            search_query: String::new(),
            selected_pid: None,
            mode: AppMode::ProcessSelect,
            timer_input: String::new(),
            timer_seconds: 0,
            timer_start: None,
            status_message: String::from("프로세스를 선택하세요 (↑↓: 이동, /: 검색, Enter: 선택)"),
        };
        app.refresh_processes();
        app
    }

    fn refresh_processes(&mut self) {
        self.system.refresh_all();
        self.processes.clear();
        
        for (pid, process) in self.system.processes() {
            let name = process.name().to_string();
            self.processes.push((*pid, name));
        }
        
        self.processes.sort_by(|a, b| a.1.cmp(&b.1));
        self.filter_processes();
        
        if !self.filtered_processes.is_empty() && self.list_state.selected().is_none() {
            self.list_state.select(Some(0));
        }
    }

    fn filter_processes(&mut self) {
        if self.search_query.is_empty() {
            self.filtered_processes = self.processes.clone();
        } else {
            let query = self.search_query.to_lowercase();
            self.filtered_processes = self
                .processes
                .iter()
                .filter(|(_, name)| name.to_lowercase().contains(&query))
                .cloned()
                .collect();
        }
        
        // 선택된 인덱스 조정
        if let Some(selected) = self.list_state.selected() {
            if selected >= self.filtered_processes.len() {
                self.list_state.select(Some(self.filtered_processes.len().saturating_sub(1)));
            }
        }
    }

    fn next(&mut self) {
        let i = match self.list_state.selected() {
            Some(i) => {
                if i >= self.filtered_processes.len().saturating_sub(1) {
                    0
                } else {
                    i + 1
                }
            }
            None => 0,
        };
        self.list_state.select(Some(i));
    }

    fn previous(&mut self) {
        let i = match self.list_state.selected() {
            Some(i) => {
                if i == 0 {
                    self.filtered_processes.len().saturating_sub(1)
                } else {
                    i - 1
                }
            }
            None => 0,
        };
        self.list_state.select(Some(i));
    }

    fn select_process(&mut self) {
        if let Some(selected) = self.list_state.selected() {
            if let Some((pid, _)) = self.filtered_processes.get(selected) {
                self.selected_pid = Some(*pid);
                self.mode = AppMode::TimerInput;
                self.status_message = format!("타이머 시간을 입력하세요 (분:초 형식, 예: 5:30 또는 300초)");
            }
        }
    }

    fn start_timer(&mut self) {
        if let Ok(seconds) = self.parse_timer_input() {
            self.timer_seconds = seconds;
            self.timer_start = Some(Instant::now());
            self.mode = AppMode::TimerRunning;
            self.status_message = format!("타이머 실행 중... (Q: 취소)");
        } else {
            self.status_message = format!("잘못된 형식입니다. 예: 5:30 또는 300");
        }
    }

    fn parse_timer_input(&self) -> Result<u64, ()> {
        let input = self.timer_input.trim();
        
        // 분:초 형식 처리 (예: 5:30)
        if let Some(colon_pos) = input.find(':') {
            let minutes: u64 = input[..colon_pos].parse().map_err(|_| ())?;
            let seconds: u64 = input[colon_pos + 1..].parse().map_err(|_| ())?;
            return Ok(minutes * 60 + seconds);
        }
        
        // 초 단위만 입력 (예: 300)
        if let Ok(seconds) = input.parse::<u64>() {
            return Ok(seconds);
        }
        
        Err(())
    }

    fn get_remaining_time(&self) -> Option<u64> {
        if let Some(start) = self.timer_start {
            let elapsed = start.elapsed().as_secs();
            if elapsed >= self.timer_seconds {
                return Some(0);
            }
            return Some(self.timer_seconds - elapsed);
        }
        None
    }

    fn kill_process(&mut self) -> bool {
        if let Some(pid) = self.selected_pid {
            self.system.refresh_process(pid);
            if let Some(process) = self.system.process(pid) {
                #[cfg(windows)]
                {
                    // Windows에서는 taskkill 명령어 사용
                    use std::process::Command;
                    let pid_u32: u32 = (*pid).into();
                    let pid_str = pid_u32.to_string();
                    let output = Command::new("taskkill")
                        .args(&["/PID", &pid_str, "/F"])
                        .output();
                    
                    if let Ok(result) = output {
                        return result.status.success();
                    }
                    return false;
                }
                #[cfg(not(windows))]
                {
                    // Unix 계열에서는 kill 시그널 사용
                    process.kill();
                    return true;
                }
            }
        }
        false
    }
}

fn main() -> io::Result<()> {
    enable_raw_mode()?;
    let mut stdout = stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let mut app = App::new();
    let mut should_quit = false;

    while !should_quit {
        terminal.draw(|f| ui(f, &app))?;
        
        // 타이머 체크
        if app.mode == AppMode::TimerRunning {
            if let Some(remaining) = app.get_remaining_time() {
                if remaining == 0 {
                    if app.kill_process() {
                        app.status_message = format!("프로세스가 종료되었습니다.");
                        app.mode = AppMode::ProcessSelect;
                        app.selected_pid = None;
                        app.timer_start = None;
                        app.refresh_processes();
                    } else {
                        app.status_message = format!("프로세스 종료 실패");
                        app.mode = AppMode::ProcessSelect;
                    }
                }
            }
        }

        should_quit = handle_events(&mut app)?;
        
        // 프로세스 목록 주기적 갱신
        if app.mode == AppMode::ProcessSelect {
            app.refresh_processes();
        }
    }

    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture
    )?;
    terminal.show_cursor()?;

    Ok(())
}

fn ui(frame: &mut Frame, app: &App) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(0),
            Constraint::Length(3),
            Constraint::Length(3),
        ])
        .split(frame.area());

    // 헤더
    let header = Paragraph::new(vec![Line::from(vec![Span::styled(
        "프로세스 종료 타이머",
        Style::default()
            .fg(Color::Cyan)
            .add_modifier(Modifier::BOLD),
    )])])
    .block(Block::default().borders(Borders::ALL).title("Header"))
    .alignment(Alignment::Center);
    frame.render_widget(header, chunks[0]);

    // 메인 영역
    match app.mode {
        AppMode::ProcessSelect => render_process_list(frame, app, chunks[1]),
        AppMode::TimerInput => render_timer_input(frame, app, chunks[1]),
        AppMode::TimerRunning => render_timer_running(frame, app, chunks[1]),
    }

    // 상태 메시지
    let status = Paragraph::new(app.status_message.as_str())
        .block(Block::default().borders(Borders::ALL).title("상태"))
        .style(Style::default().fg(Color::Yellow))
        .wrap(Wrap { trim: true });
    frame.render_widget(status, chunks[2]);

    // 도움말
    let help_text = match app.mode {
        AppMode::ProcessSelect => "↑↓: 이동 | /: 검색 | Enter: 선택 | Q: 종료",
        AppMode::TimerInput => "분:초 형식 입력 (예: 5:30) | Enter: 시작 | Esc: 취소",
        AppMode::TimerRunning => "Q: 타이머 취소",
    };
    let help = Paragraph::new(help_text)
        .block(Block::default().borders(Borders::ALL).title("도움말"))
        .style(Style::default().fg(Color::Gray))
        .alignment(Alignment::Center);
    frame.render_widget(help, chunks[3]);
}

fn render_process_list(frame: &mut Frame, app: &App, area: Rect) {
    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(70), Constraint::Percentage(30)])
        .split(area);

    // 프로세스 목록
    let items: Vec<ListItem> = app
        .filtered_processes
        .iter()
        .enumerate()
        .map(|(i, (pid, name))| {
            let is_selected = app.list_state.selected() == Some(i);
            let style = if is_selected {
                Style::default()
                    .fg(Color::Black)
                    .bg(Color::White)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::White)
            };
            
            let pid_u32: u32 = (*pid).into();
            let content = format!("[{}] {}", pid_u32, name);
            ListItem::new(content).style(style)
        })
        .collect();

    let list = List::new(items)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title("프로세스 목록"),
        )
        .highlight_style(Style::default().add_modifier(Modifier::BOLD));
    
    frame.render_stateful_widget(list, chunks[0], &mut app.list_state.clone());

    // 검색 영역
    let search_label = if app.search_query.is_empty() {
        "검색어 입력 (/ 입력 후 검색)"
    } else {
        app.search_query.as_str()
    };
    let search = Paragraph::new(search_label)
        .block(Block::default().borders(Borders::ALL).title("검색"))
        .style(if app.search_query.is_empty() {
            Style::default().fg(Color::Gray)
        } else {
            Style::default().fg(Color::Green)
        });
    frame.render_widget(search, chunks[1]);
}

fn render_timer_input(frame: &mut Frame, app: &App, area: Rect) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(3), Constraint::Min(0)])
        .split(area);

    let selected_process = if let Some(pid) = app.selected_pid {
        app.processes
            .iter()
            .find(|(p, _)| *p == pid)
            .map(|(_, name)| name.clone())
            .unwrap_or_else(|| "알 수 없음".to_string())
    } else {
        "없음".to_string()
    };

    let info = Paragraph::new(vec![
        Line::from(vec![Span::styled(
            format!("선택된 프로세스: {}", selected_process),
            Style::default().fg(Color::Cyan),
        )]),
        Line::from(""),
        Line::from(vec![Span::styled(
            format!("타이머 입력: {}", app.timer_input),
            Style::default().fg(Color::Green),
        )]),
    ])
    .block(Block::default().borders(Borders::ALL).title("타이머 설정"))
    .wrap(Wrap { trim: true });
    
    frame.render_widget(info, chunks[0]);
}

fn render_timer_running(frame: &mut Frame, app: &App, area: Rect) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(3), Constraint::Min(0)])
        .split(area);

    let selected_process = if let Some(pid) = app.selected_pid {
        app.processes
            .iter()
            .find(|(p, _)| *p == pid)
            .map(|(_, name)| name.clone())
            .unwrap_or_else(|| "알 수 없음".to_string())
    } else {
        "없음".to_string()
    };

    let remaining = app.get_remaining_time().unwrap_or(0);
    let minutes = remaining / 60;
    let seconds = remaining % 60;
    let time_str = format!("{:02}:{:02}", minutes, seconds);

    let info = Paragraph::new(vec![
        Line::from(vec![Span::styled(
            format!("프로세스: {}", selected_process),
            Style::default().fg(Color::Cyan),
        )]),
        Line::from(""),
        Line::from(vec![Span::styled(
            format!("남은 시간: {}", time_str),
            Style::default()
                .fg(Color::Red)
                .add_modifier(Modifier::BOLD),
        )]),
    ])
    .block(Block::default().borders(Borders::ALL).title("타이머 실행 중"))
    .wrap(Wrap { trim: true })
    .alignment(Alignment::Center);
    
    frame.render_widget(info, chunks[0]);
}

fn handle_events(app: &mut App) -> io::Result<bool> {
    if event::poll(Duration::from_millis(100))? {
        if let Event::Key(key) = event::read()? {
            if key.kind == KeyEventKind::Press {
                match app.mode {
                    AppMode::ProcessSelect => {
                        match key.code {
                            KeyCode::Char('q') | KeyCode::Char('Q') => return Ok(true),
                            KeyCode::Up => app.previous(),
                            KeyCode::Down => app.next(),
                            KeyCode::Char('/') => {
                                app.search_query.clear();
                            }
                            KeyCode::Enter => app.select_process(),
                            KeyCode::Char(c) => {
                                app.search_query.push(c);
                                app.filter_processes();
                            }
                            KeyCode::Backspace => {
                                app.search_query.pop();
                                app.filter_processes();
                            }
                            _ => {}
                        }
                    }
                    AppMode::TimerInput => {
                        match key.code {
                            KeyCode::Char('q') | KeyCode::Char('Q') => return Ok(true),
                            KeyCode::Esc => {
                                app.mode = AppMode::ProcessSelect;
                                app.selected_pid = None;
                                app.timer_input.clear();
                                app.status_message = String::from("프로세스를 선택하세요");
                            }
                            KeyCode::Enter => app.start_timer(),
                            KeyCode::Char(c) => {
                                app.timer_input.push(c);
                            }
                            KeyCode::Backspace => {
                                app.timer_input.pop();
                            }
                            _ => {}
                        }
                    }
                    AppMode::TimerRunning => {
                        match key.code {
                            KeyCode::Char('q') | KeyCode::Char('Q') => {
                                app.mode = AppMode::ProcessSelect;
                                app.selected_pid = None;
                                app.timer_start = None;
                                app.status_message = String::from("타이머가 취소되었습니다");
                            }
                            _ => {}
                        }
                    }
                }
            }
        }
    }
    Ok(false)
}
