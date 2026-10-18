//! `emrac ui` (u17): every element of the look, in this terminal and theme.

use std::time::Duration;

use emrac_core::style::{
    bar, boxed, cell, diff_line, heading, line_text, mark, render_line, sparkline, version_change,
    Activity, Animations, Column, Depth, Role, Seg, Status, Style, Table, Theme,
};
use emrac_core::Result;

fn yes_no(b: bool) -> &'static str {
    if b {
        "yes"
    } else {
        "no"
    }
}

/// The gallery as text, for the style `st`.
pub fn gallery(st: &Style, animations: Animations) -> String {
    let mut out = String::new();
    let g = st.glyphs();
    out.push_str(&format!(
        "{}\n\n",
        st.paint(Role::Brand, &format!("emrac {} how it looks", g.bullet))
    ));

    out.push_str(&format!("{}\n", heading(st, "This terminal")));
    let depth = match st.depth {
        Depth::Basic => "16 colours",
        Depth::Color256 => "256 colours",
        Depth::TrueColor => "true colour",
    };
    for (k, v) in [
        ("colour", format!("{} ({depth})", yes_no(st.color))),
        ("unicode", yes_no(st.unicode).to_string()),
        (
            "layout",
            if st.fancy {
                "marks, boxes and tables"
            } else {
                "plain text (not a terminal, or --json/--quiet)"
            }
            .to_string(),
        ),
        ("width", format!("{} columns", st.width)),
        ("theme", st.theme.word().to_string()),
        (
            "animations",
            match animations {
                Animations::Regular => "regular (nothing moves; set clui.animations active)",
                Animations::Active => "active",
            }
            .to_string(),
        ),
    ] {
        out.push_str(&format!(
            "  {}  {v}\n",
            st.paint(Role::Dim, &emrac_core::style::pad_right(k, 11))
        ));
    }

    out.push_str(&format!("\n{}\n", heading(st, "Roles")));
    let roles = [
        (Role::Heading, "heading"),
        (Role::Package, "package"),
        (Role::Version, "version"),
        (Role::Size, "size"),
        (Role::Command, "command"),
        (Role::Path, "path"),
        (Role::Added, "added"),
        (Role::Removed, "removed"),
        (Role::Accent, "accent"),
        (Role::Note, "note"),
        (Role::Dim, "dim"),
        (Role::Ok, "ok"),
        (Role::Warn, "warn"),
        (Role::Problem, "problem"),
        (Role::Question, "question"),
    ];
    for chunk in roles.chunks(8) {
        let row: Vec<String> = chunk.iter().map(|(r, n)| st.paint(*r, n)).collect();
        out.push_str(&format!("  {}\n", row.join("  ")));
    }

    out.push_str(&format!("\n{}\n", heading(st, "Marks")));
    for s in [Status::Ok, Status::Note, Status::Warning, Status::Problem] {
        out.push_str(&format!(" {}\n", render_line(st, &mark(st, s))));
    }

    out.push_str(&format!("\n{}\n", heading(st, "A box")));
    let lines = vec![
        vec![
            Seg::new("extra", Role::Dim),
            Seg::plain("  "),
            Seg::new("vim", Role::Package),
            Seg::plain("  "),
            Seg::new("9.1.1-1", Role::Version),
        ],
        vec![
            Seg::new("extra", Role::Dim),
            Seg::plain("  "),
            Seg::new("gpm", Role::Package),
            Seg::plain("  "),
            Seg::new("1.20.7-5", Role::Version),
            Seg::new("  dependency", Role::Dim),
        ],
    ];
    out.push_str(&boxed(
        st,
        &format!("install {} 2 packages", g.bullet),
        &lines,
    ));

    out.push_str(&format!("\n{}\n", heading(st, "A table")));
    let mut t = Table::new(vec![
        Column::new("package").flex(),
        Column::new("size").right(),
        Column::new("share"),
    ]);
    for (n, size, f) in [
        ("llvm-libs", "148.00 MiB", 0.9),
        ("mesa", "52.40 MiB", 0.35),
        ("zlib", "0.30 MiB", 0.01),
    ] {
        t.row(vec![
            cell(n, Role::Package),
            cell(size, Role::Size),
            bar(st, f, 10),
        ]);
    }
    for l in t.render(st, st.width.saturating_sub(2)).lines() {
        out.push_str(&format!("  {l}\n"));
    }

    out.push_str(&format!(
        "\n{}\n",
        heading(st, "Versions, bars, sparklines")
    ));
    out.push_str(&format!(
        "  {}\n",
        render_line(st, &version_change(st, "24.2.1-1", "24.2.3-1"))
    ));
    out.push_str(&format!(
        "  {}\n",
        render_line(st, &version_change(st, "1.2.9-1", "1.2.10-1"))
    ));
    out.push_str(&format!(
        "  {}  {}\n",
        render_line(st, &bar(st, 0.35, 20)),
        st.paint(Role::Dim, "35% of 20 cells")
    ));
    out.push_str(&format!(
        "  {}  {}\n",
        render_line(st, &sparkline(st, &[3, 5, 2, 9, 7, 4, 8, 1])),
        st.paint(Role::Dim, "last 8 builds")
    ));

    out.push_str(&format!("\n{}\n", heading(st, "A diff")));
    for l in [
        "--- a/PKGBUILD",
        "+++ b/PKGBUILD",
        "@@ -3,2 +3,2 @@",
        "-pkgver=1.2.9",
        "+pkgver=1.2.10",
        " pkgrel=1",
    ] {
        out.push_str(&format!("  {}\n", diff_line(st, l)));
    }

    out.push_str(&format!("\n{}\n", heading(st, "The voice")));
    for v in emrac_core::Voice::ALL {
        out.push_str(&format!(
            "  {} {}\n",
            st.paint(v.role(), &format!("emrac {}:", v.word())),
            st.paint(Role::Dim, &sample_for(v))
        ));
    }
    let _ = line_text;
    out
}

fn sample_for(v: emrac_core::Voice) -> String {
    use emrac_core::Voice::*;
    match v {
        Says => "something broke underneath emrac",
        Found => "a fact you can act on",
        Notes => "plain information",
        Plans => "what it is about to do",
        Asks => "a question that needs your answer",
        Warns => "a real risk",
        Suspects => "a guess from weak evidence",
        Thinks => "a judgement with its reasons",
        Suggests => "the next command to run",
        Refuses => "a refusal with no override",
        Confirms => "something checked or done",
        Remembers => "something from what you did before",
    }
    .to_string()
}

pub fn run(theme: Option<&str>, demo: bool) -> Result<()> {
    let mut st = Style::stdout();
    if let Some(name) = theme {
        match Theme::parse(name) {
            Some(t) => st.theme = t,
            None => {
                return Err(emrac_core::Error::Help(format!(
                    "'{name}' is not a theme; the themes are default, light, mono and high-contrast"
                )))
            }
        }
    }
    print!("{}", gallery(&st, emrac_core::style::settings().animations));
    if demo {
        println!("\n{}", heading(&st, "Animations (about three seconds)"));
        let a = Activity::demo("a wait of unknown length", None);
        std::thread::sleep(Duration::from_millis(1400));
        a.finish("done waiting");
        let p = Activity::demo("a wait with a count", Some(8));
        for i in 0..=8 {
            p.set(i, "step by step");
            std::thread::sleep(Duration::from_millis(200));
        }
        p.finish("counted to 8");
        if !Style::stderr().fancy {
            println!("(no terminal on standard error, so nothing was drawn)");
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use emrac_core::style::{display_width, strip_ansi};

    fn st(theme: Theme, color: bool, unicode: bool) -> Style {
        Style {
            color,
            unicode,
            fancy: true,
            width: 80,
            theme,
            depth: Depth::TrueColor,
        }
    }

    #[test]
    fn the_gallery_shows_every_part_in_every_theme_and_fits_the_width() {
        for theme in Theme::ALL {
            let g = gallery(&st(theme, false, true), Animations::Regular);
            for part in [
                "This terminal",
                "Roles",
                "Marks",
                "A box",
                "A table",
                "Versions, bars, sparklines",
                "A diff",
                "The voice",
            ] {
                assert!(g.contains(part), "{theme:?}: {part}");
            }
            for v in emrac_core::Voice::ALL {
                assert!(
                    g.contains(&format!("emrac {}:", v.word())),
                    "{theme:?} {v:?}"
                );
            }
            for l in g.lines() {
                assert!(display_width(l) <= 80, "{l:?}");
            }
            // colour only decorates; the one line that says whether colour is on differs
            let coloured = strip_ansi(&gallery(&st(theme, true, true), Animations::Regular));
            assert_eq!(
                coloured.replace("colour       yes", "colour       no"),
                g,
                "{theme:?}"
            );
        }
    }

    #[test]
    fn the_ascii_gallery_is_ascii() {
        let g = gallery(&st(Theme::Default, false, false), Animations::Active);
        assert!(
            g.is_ascii(),
            "{}",
            g.chars()
                .find(|c| !c.is_ascii())
                .map(|c| c.to_string())
                .unwrap_or_default()
        );
        assert!(g.contains("animations   active"));
        assert!(g.contains("unicode      no"));
    }

    #[test]
    fn the_gallery_says_what_regular_means() {
        let g = gallery(&st(Theme::Mono, false, true), Animations::Regular);
        assert!(g.contains("regular (nothing moves; set clui.animations active)"));
        assert!(g.contains("theme        mono"));
    }
}
