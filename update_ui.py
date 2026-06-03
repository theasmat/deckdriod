import re

with open("src/main.rs", "r") as f:
    content = f.read()

# We need to replace the layout setup at the top of ui()
old_setup = """    let has_input = matches!(app.state.mode, AppMode::Search | AppMode::Input | AppMode::DeepLink);
    // header(1) + tabs(1) + separator(1) + [input(3)] + main(min) + footer(1)
    let mut main_constraints = vec![
        Constraint::Length(1),  // header
        Constraint::Length(1),  // tabs
        Constraint::Length(1),  // separator line
        Constraint::Min(0),     // main content
        Constraint::Length(1),  // footer
    ];
    if has_input { main_constraints.insert(3, Constraint::Length(3)); }
    let chunks = Layout::default().direction(Direction::Vertical).constraints(main_constraints).split(area);
    let mut current_idx = 0;

    {
        let bat = match app.state.stats.battery_level {
            Some(l) => Span::styled(format!(" BAT:{}%", l), if l < 20 { Style::default().fg(Color::Red).bold() } else { Style::default().fg(Color::Green) }),
            None => Span::styled(" BAT:--%", Style::default().fg(Color::DarkGray)),
        };
        let device_str = app.state.device_serial.as_deref().unwrap_or("no device");
        let max_id = (area.width as usize).saturating_sub(70);
        let app_id = &app.config.app_id;
        let app_id_display = if app_id.len() > max_id && max_id > 3 {
            format!("{}...", &app_id[..max_id.saturating_sub(3)])
        } else { app_id.clone() };

        let mut spans = vec![
            Span::styled(" DeckDriod ", Style::default().bold().fg(Color::Cyan)),
            Span::styled(format!("v{} ", env!("CARGO_PKG_VERSION")), Style::default().fg(Color::DarkGray)),
            Span::styled(format!(" {} ", device_str), Style::default().fg(Color::White)),
            bat,
            Span::styled(format!("  CPU:{:.0}%", app.state.stats.last_cpu), Style::default().fg(Color::Green)),
            Span::styled(format!("  MEM:{:.0}%", app.state.stats.last_mem), Style::default().fg(Color::Blue)),
            Span::styled("  |", Style::default().fg(Color::DarkGray)),
            Span::styled(format!(" Watch:{}", if app.state.auto_rebuild { "ON" } else { "OFF" }), if app.state.auto_rebuild { Style::default().fg(Color::Green) } else { Style::default().fg(Color::DarkGray) }),
            Span::styled(format!(" Open:{}", if app.state.auto_open { "ON" } else { "OFF" }), if app.state.auto_open { Style::default().fg(Color::Green) } else { Style::default().fg(Color::DarkGray) }),
            Span::styled("  |", Style::default().fg(Color::DarkGray)),
        ];
        if app.state.is_broadcast { spans.push(Span::styled(" [BROADCAST]", Style::default().bg(Color::Red).fg(Color::White).bold())); }
        if app.state.mcp_server_active { spans.push(Span::styled(format!(" [MCP:{}]", app.state.mcp_port), Style::default().bg(Color::Blue).fg(Color::White).bold())); }
        if app.state.is_recording { spans.push(Span::styled(" [REC]", Style::default().bg(Color::Red).fg(Color::White).bold())); }
        spans.push(Span::styled(format!("  {}", app_id_display), Style::default().fg(Color::DarkGray)));

        f.render_widget(Paragraph::new(Line::from(spans)), chunks[current_idx]);
        current_idx += 1;
    }

    {
        let tab_titles = vec![
            " [1] Dashboard ".to_string(),
            format!(" [2] App ({}) ", app.cache_app.len()),
            format!(" [3] Build ({}) ", app.cache_build.len()),
            format!(" [4] Errors ({}) ", app.cache_err.len()),
        ];
        let selected = match app.state.current_tab { Tab::Dashboard => 0, Tab::App => 1, Tab::Build => 2, Tab::Errors => 3 };
        let tabs = Tabs::new(tab_titles)
            .select(selected)
            .style(Style::default().fg(Color::DarkGray))
            .highlight_style(Style::default().fg(Color::Black).bg(Color::Cyan).bold());
        f.render_widget(tabs, chunks[current_idx]);
        current_idx += 1;
    }

    // separator line
    f.render_widget(
        Paragraph::new(Line::from(Span::styled(
            "─".repeat(area.width as usize),
            Style::default().fg(Color::DarkGray),
        ))),
        chunks[current_idx],
    );
    current_idx += 1;

    if has_input {
        let title = match app.state.mode { AppMode::Search => " / Search ", AppMode::DeepLink => " Deep Link URL ", _ => " Input " };
        f.render_widget(
            Paragraph::new(format!("{}_", app.state.input_buffer))
                .block(Block::default().borders(Borders::ALL).border_type(BorderType::Rounded).title(title).border_style(Style::default().fg(Color::Yellow))),
            chunks[current_idx],
        );
        current_idx += 1;
    }

    let main_area = chunks[current_idx];"""

new_setup = """    let outer_chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Length(32), Constraint::Min(0)])
        .split(area);
        
    let sidebar_area = outer_chunks[0];
    let content_area = outer_chunks[1];

    let sidebar_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(11), // Status
            Constraint::Length(6),  // Tabs
            Constraint::Min(0),     // Spacer
            Constraint::Length(16), // Shortcuts
        ])
        .split(sidebar_area);

    let has_input = matches!(app.state.mode, AppMode::Search | AppMode::Input | AppMode::DeepLink);
    let mut content_constraints = vec![Constraint::Min(0)];
    if has_input { content_constraints.insert(0, Constraint::Length(3)); }
    let content_chunks = Layout::default().direction(Direction::Vertical).constraints(content_constraints).split(content_area);

    let mut current_idx = 0;
    if has_input {
        let title = match app.state.mode { AppMode::Search => " / Search ", AppMode::DeepLink => " Deep Link URL ", _ => " Input " };
        f.render_widget(
            Paragraph::new(format!("{}_", app.state.input_buffer))
                .block(Block::default().borders(Borders::ALL).border_type(BorderType::Rounded).title(title).border_style(Style::default().fg(Color::Yellow))),
            content_chunks[current_idx],
        );
        current_idx += 1;
    }
    let main_area = content_chunks[current_idx];

    // -- Render Status Block (sidebar_chunks[0]) --
    {
        let bat = match app.state.stats.battery_level {
            Some(l) => Span::styled(format!("BAT:{}%", l), if l < 20 { Style::default().fg(Color::Red).bold() } else { Style::default().fg(Color::Green) }),
            None => Span::styled("BAT:--%", Style::default().fg(Color::DarkGray)),
        };
        let device_str = app.state.device_serial.as_deref().unwrap_or("no device");
        
        let mut lines = vec![
            Line::from(vec![Span::styled(" DeckDriod ", Style::default().bold().fg(Color::Cyan)), Span::styled(format!("v{}", env!("CARGO_PKG_VERSION")), Style::default().fg(Color::DarkGray))]),
            Line::from(vec![Span::raw("")]),
            Line::from(vec![Span::styled(" DEVICE:", Style::default().fg(Color::DarkGray))]),
            Line::from(vec![Span::styled(format!(" {}", device_str), Style::default().fg(Color::White))]),
            Line::from(vec![Span::styled(format!(" CPU: {:.0}%", app.state.stats.last_cpu), Style::default().fg(Color::Green))]),
            Line::from(vec![Span::styled(format!(" MEM: {:.0}%", app.state.stats.last_mem), Style::default().fg(Color::Blue))]),
            Line::from(vec![Span::styled(" ", Style::default()), bat]),
            Line::from(vec![Span::styled(format!(" Watch: {}", if app.state.auto_rebuild { "ON" } else { "OFF" }), if app.state.auto_rebuild { Style::default().fg(Color::Green) } else { Style::default().fg(Color::DarkGray) })]),
            Line::from(vec![Span::styled(format!(" Open: {}", if app.state.auto_open { "ON" } else { "OFF" }), if app.state.auto_open { Style::default().fg(Color::Green) } else { Style::default().fg(Color::DarkGray) })]),
        ];
        f.render_widget(Paragraph::new(lines).block(Block::default().borders(Borders::ALL).border_type(BorderType::Rounded).title(" Status ").border_style(Style::default().fg(Color::Cyan))), sidebar_chunks[0]);
    }

    // -- Render Tabs (sidebar_chunks[1]) --
    {
        let selected = match app.state.current_tab { Tab::Dashboard => 0, Tab::App => 1, Tab::Build => 2, Tab::Errors => 3 };
        let items = vec![
            ListItem::new(format!("{} [1] Dashboard", if selected == 0 { "▶" } else { " " })).style(if selected == 0 { Style::default().fg(Color::Cyan).bold() } else { Style::default().fg(Color::DarkGray) }),
            ListItem::new(format!("{} [2] App Logs ({})", if selected == 1 { "▶" } else { " " }, app.cache_app.len())).style(if selected == 1 { Style::default().fg(Color::Cyan).bold() } else { Style::default().fg(Color::DarkGray) }),
            ListItem::new(format!("{} [3] Build Logs ({})", if selected == 2 { "▶" } else { " " }, app.cache_build.len())).style(if selected == 2 { Style::default().fg(Color::Cyan).bold() } else { Style::default().fg(Color::DarkGray) }),
            ListItem::new(format!("{} [4] Errors ({})", if selected == 3 { "▶" } else { " " }, app.cache_err.len())).style(if selected == 3 { Style::default().fg(Color::Red).bold() } else { Style::default().fg(Color::DarkGray) }),
        ];
        f.render_widget(List::new(items).block(Block::default().borders(Borders::ALL).border_type(BorderType::Rounded).title(" Views ").border_style(Style::default().fg(Color::DarkGray))), sidebar_chunks[1]);
    }

    // -- Render Shortcuts (sidebar_chunks[3]) --
    {
        let shortcuts = vec![
            Line::from(vec![Span::styled(" r", Style::default().fg(Color::Cyan).bold()), Span::raw(" Run")]),
            Line::from(vec![Span::styled(" d", Style::default().fg(Color::Cyan).bold()), Span::raw(" Device")]),
            Line::from(vec![Span::styled(" c", Style::default().fg(Color::Cyan).bold()), Span::raw(" Clear")]),
            Line::from(vec![Span::styled(" t", Style::default().fg(Color::Cyan).bold()), Span::raw(" Variant")]),
            Line::from(vec![Span::styled(" m", Style::default().fg(Color::Cyan).bold()), Span::raw(" MCP Toggle")]),
            Line::from(vec![Span::styled(" b", Style::default().fg(Color::Cyan).bold()), Span::raw(" Bounds")]),
            Line::from(vec![Span::styled(" v", Style::default().fg(Color::Cyan).bold()), Span::raw(" Video")]),
            Line::from(vec![Span::styled(" s", Style::default().fg(Color::Cyan).bold()), Span::raw(" Screenshot")]),
            Line::from(vec![Span::styled(" y", Style::default().fg(Color::Cyan).bold()), Span::raw(" Yank")]),
            Line::from(vec![Span::styled(" /", Style::default().fg(Color::Cyan).bold()), Span::raw(" Search")]),
            Line::from(vec![Span::styled(" h", Style::default().fg(Color::Cyan).bold()), Span::raw(" Help")]),
            Line::from(vec![Span::styled(" q", Style::default().fg(Color::Cyan).bold()), Span::raw(" Quit")]),
        ];
        f.render_widget(Paragraph::new(shortcuts).block(Block::default().borders(Borders::ALL).border_type(BorderType::Rounded).title(" Shortcuts ").border_style(Style::default().fg(Color::DarkGray))), sidebar_chunks[3]);
    }
"""

# Now we need to remove the old footer rendering at the bottom of ui()
old_footer = """    let crash_span = if app.state.last_crash.is_some() {
        Span::styled(" [CRASH] ", Style::default().bg(Color::Red).fg(Color::White).bold())
    } else { Span::raw("") };
    let build_span = if let Some(ref t) = app.state.build_task {
        let short = if t.len() > 30 { format!("{}...", &t[..29]) } else { t.clone() };
        Span::styled(format!(" [BUILD: {}] ", short), Style::default().fg(Color::Yellow).bold())
    } else { Span::raw("") };
    let footer_line = Line::from(vec![
        Span::styled(" › Press ", Style::default().fg(Color::DarkGray)),
        Span::styled("r", Style::default().fg(Color::Cyan).bold()),
        Span::styled(" reload | ", Style::default().fg(Color::DarkGray)),
        Span::styled("d", Style::default().fg(Color::Cyan).bold()),
        Span::styled(" device | ", Style::default().fg(Color::DarkGray)),
        Span::styled("c", Style::default().fg(Color::Cyan).bold()),
        Span::styled(" clear | ", Style::default().fg(Color::DarkGray)),
        Span::styled("t", Style::default().fg(Color::Cyan).bold()),
        Span::styled(" variant | ", Style::default().fg(Color::DarkGray)),
        Span::styled("h", Style::default().fg(Color::Cyan).bold()),
        Span::styled(" help | ", Style::default().fg(Color::DarkGray)),
        Span::styled("q", Style::default().fg(Color::Cyan).bold()),
        Span::styled(" quit ", Style::default().fg(Color::DarkGray)),
        Span::raw("  "),
        build_span,
        crash_span,
    ]);
    f.render_widget(Paragraph::new(footer_line), chunks[chunks.len() - 1]);"""

new_content = content.replace(old_setup, new_setup)
new_content = new_content.replace(old_footer, "")

if new_content == content:
    print("Error: Regex/Replace did not match anything.")
else:
    with open("src/main.rs", "w") as f:
        f.write(new_content)
    print("Successfully replaced layout code.")
