# Acceptance evidence

Every requirement of [the briefing](BRIEFING.md) and its amendments, with what shows it. Evidence is a test (`file: name`), a sample with the page inspected, or a command. **Missing** means no evidence exists yet. Nothing here narrows the briefing. The state of the release gate is in [progress](PROGRESS.md).

Test files: `src/<module>` unit tests, `tests/cli.rs` (CLI), `tests/acceptance.rs` (every sample), `tests/parity.rs`, `tests/public_api.rs`. Visual evidence is from the 2026-11-03 pass in [progress](PROGRESS.md#verification), which does not replace the owner's review.

## Gaps

- **Linux and Windows:** layout identity across platforms, `cargo test`, and the sample renders were never run. CI covers them, but no workflow has run because nothing is pushed.
- **Owner's visual review:** not done.
- **Continuing footnote in the corpus:** no sample shows one. `samples/pagination.md` says note 3 continues, but it fits on page 2. `layout/pages.rs: a_long_note_continues_on_the_next_page` covers the behavior.
- **Conformance validators:** veraPDF (PDF/A, PDF/UA), epubcheck, Ace, and screen readers were never run.
- **Performance limits are not enforced** by a test or CI.
- **Phase 2 on other platforms:** parity, the examples, the doctests, and the API tests ran on macOS only. The `Examples` and `Documentation` steps of the CI workflow are new and have not run.
- **Phase 2, a second painter:** only `druck-desktop` draws from a `Layout`.
- **Phase 2, no printing and no filesystem in the core** is checked by a grep run by hand, not by a test.
- **Phase 2, mouse clicks on a page:** automated mouse events did not reach the window on macOS. The click was proven through gpui's dispatch, and a real mouse click is untried.
- **Windows offline run:** `scripts/check-offline.sh` has no Windows branch.
- **Desktop notices:** 41 crates have no license file, one is MPL-2.0.

## Test files by name

| Short | Means |
| --- | --- |
| acceptance | `tests/acceptance.rs: every_sample_passes_the_acceptance_checks`, run over every sample and each sample's theme: overflow (nothing drawn outside its page), lost content (every long word of the prose is on a page), references (every link, anchor, and bookmark has a placed destination, no `??`), stability (two renders write the same PDF), PDF features (fonts embedded with a text map, link annotations equal links, outline present) |
| samples | `scripts/render-samples.sh` then `scripts/check-pdfs.sh` (fonts embedded, Unicode maps, text extracts) |
| offline | `scripts/check-offline.sh` |

## Authoring contract

| Requirement | Evidence |
| --- | --- |
| CommonMark baseline | `markdown.rs: parses_headings_paragraphs_quotes_and_code`, `parses_ordered_and_tight_bullet_lists`, `flattens_nested_inline_styles` |
| Tables, footnotes, citations, captions and labels, cross-references, directives | `markdown.rs: parses_tables_with_alignment_styles_and_padded_cells`, `numbers_footnotes_in_reference_order`, `parses_citations_with_their_key_locations_and_adds_the_bibliography_at_the_end`, `parses_images_alone_in_a_paragraph_with_the_description_as_caption`, `parses_labels_and_references_and_leaves_escaped_at_signs_as_text`, `parses_directives_between_blocks_but_not_inside_code` |
| YAML front matter and limited settings | `config/front_matter.rs: maps_settings_onto_fixed_theme_paths`, `rejects_null_unknown_keys_and_template_fields`; `cli.rs: reports_yaml_errors_at_the_document_line` |
| Document settings cannot redefine templates | `config/front_matter.rs: rejects_null_unknown_keys_and_template_fields` |
| Layout directives: page break, keep, columns, full-width | `layout/pages.rs: keeps_a_group_on_one_page_and_honours_explicit_breaks`; `layout/tests.rs: a_keep_group_inside_columns_stays_in_one_column`, `a_full_width_block_balances_the_columns_before_it_and_columns_resume_below` |
| Unsupported constructs get clear diagnostics, HTML not rendered | `markdown.rs: reports_each_unsupported_construct`, `reports_unterminated_comments_at_their_start_and_other_html_at_its_own_location`; `cli.rs: reports_unsupported_content_with_its_location_and_writes_nothing` |

## Theme architecture

| Requirement | Evidence |
| --- | --- |
| One versioned JSON format, bundled complete default, partial themes | `config/schema_tests.rs: default_theme_deserializes_and_matches_the_schema`; `cli.rs: loads_a_document_with_a_partial_theme_and_tracks_resource_origins` |
| Resolution order, later wins | `cli.rs: later_layers_override_earlier_ones_and_the_theme_flag_beats_front_matter` |
| Nested object merge, whole-array replacement, omitted fields | `config/merge.rs: objects_merge_by_field_and_arrays_replace_whole`; `cli.rs: objects_merge_by_field_arrays_replace_and_permitted_nulls_are_kept` |
| Null permitted and rejected | `cli.rs: rejects_null_where_the_type_does_not_allow_it`; `config/merge.rs: null_replaces_the_earlier_value_for_typed_validation_to_judge` |
| No parent chains or overlays | Not constructible: one `--theme`, no `extends` field (`schema_and_default_theme_declare_the_same_fields`) |
| All layers valid, resolved config validated | `config/schema_tests.rs: schema_and_layer_check_agree_on_partial_themes`; `config/resolve.rs` rejection tests |
| Tokens resolve, missing and cyclic references rejected | `cli.rs: rejects_undefined_and_cyclic_token_references`; `config/values.rs: token_references_must_match_the_field_group` |
| Lengths pt, mm, cm, in, em, unitless line height; no percent or viewport | `config/values.rs: lengths_convert_absolute_units_and_keep_em_relative`, `lengths_reject_missing_unknown_and_negative_units`; `cli.rs: resolves_em_against_the_body_size_and_rejects_other_units` |
| Title page, header, footer slots; values as text; omit empty; required missing is an error | `layout/structure_tests.rs: a_title_block_omits_empty_optional_slots_with_their_spacing_and_keeps_metadata_as_text`, `a_required_title_slot_without_a_value_is_an_error`; `config/template.rs: fills_values_as_text_and_names_the_first_missing_one`; `cli.rs: check_reports_a_required_title_slot_without_a_value` |
| Page variants, fallback order, parity | `layout/structure_tests.rs: page_variants_follow_the_fallback_order_and_physical_parity`, `a_title_page_stands_alone_and_body_pages_follow_its_parity` |
| Variant and section-title selection agree across layout and PDF | `layout/structure_tests.rs: headers_show_the_section_and_subsection_of_each_page_and_footers_the_page`; sample `report` (default and custom theme), inspected |
| Schema published, typed, versioned; unknown fields, versions, values rejected | `config/schema_tests.rs`; `cli.rs: rejects_wrong_or_missing_versions_and_unknown_fields` |
| Source and property named | `cli.rs: reports_a_missing_theme_font_relative_to_the_theme`, `shows_normalized_paths_relative_to_the_working_directory` |
| Theme assets relative to the theme; document assets to the document; from another working directory | `cli.rs: named_themes_load_from_the_os_config_directory_with_their_asset_origin`, `uses_font_files_relative_to_the_document`, `renders_images_relative_to_the_document_the_same_every_time`, `set_paths_resolve_against_the_working_directory`; every `cli.rs` run starts in an empty working directory |
| Local and offline theme loading | offline |
| Distributing a theme with its assets documented | [themes](THEMES.md) |

## Content and typography

| Requirement | Evidence |
| --- | --- |
| Paragraphs, headings, lists, quotations, code, images, tables, footnotes, citations | samples `en`, `de`, `report`, `report-de`, `tables`, `images`, all inspected; acceptance |
| Math excluded | Not implemented, by the briefing |
| Optimal line breaking on shaped metrics | `layout/paragraph/breaking.rs: looks_ahead_where_greedy_filling_strands_a_word`, `prefers_the_fuller_line_when_no_line_fits_within_tolerance` |
| Kerning and ligatures | `text/tests.rs: kerning_narrows_a_pair`, `fi_ligature_is_one_glyph_over_both_bytes` |
| English and German hyphenation | `layout/paragraph/hyphenation.rs: hyphenates_english_words_and_keeps_punctuation_whole`, `hyphenates_german_compounds` |
| Optical margin alignment | `layout/tests.rs: justified_lines_fill_the_measure_with_punctuation_hanging_into_the_margins`, `a_run_of_punctuation_hangs_by_the_sum_of_its_shares` |
| Scholarly default design | Sample `report` in the default theme, inspected; owner's review missing |
| Bundled fonts and hyphenation, no system resources | `text/tests.rs: bundled_faces_cover_german_and_english_text`; offline (empty home, no config) |

## Page composition and columns

| Requirement | Evidence |
| --- | --- |
| Scored breaks, real heights | `layout/pages.rs: a_note_moves_the_break_before_its_reference`; `layout/tests.rs: a_heading_keeps_two_lines_of_its_paragraph_wants_a_third_and_pages_strand_no_line`, `a_page_break_next_to_a_one_line_paragraph_costs_much` |
| Heading kept with text, no stranded lines | `layout/pages.rs: a_heading_near_the_bottom_moves_with_its_text`, `avoids_widows_and_orphans`, `a_page_strands_no_line_even_when_it_runs_short` |
| Keep groups, conflicts reported | `layout/tests.rs: reports_a_keep_group_taller_than_the_page_at_its_directive`, `reports_a_keep_group_taller_than_a_column_at_its_directive` |
| Bounded spacing, short last pages | `layout/tests.rs: reports_the_rules_a_page_breaks_as_warnings_naming_the_page`; `druck check samples/report.md` lists pages that end short |
| Mid-page 1-2-1 columns, gap configurable | `layout/tests.rs: changing_from_one_to_two_columns_and_back_does_not_start_a_new_page`, `text_flows_down_the_first_column_then_the_second`; sample `columns` p1 |
| Sections across pages, balancing | `layout/tests.rs: a_column_section_continues_across_pages_with_full_columns`, `the_final_columns_of_a_section_are_balanced`; sample `columns` p1 to p2, `report` p3 to p5 |
| Full-width blocks | `layout/tests.rs: a_full_width_image_sits_between_balanced_column_regions`, `a_full_width_table_sits_between_balanced_columns_that_resume_below`; samples `images` p2, `tables` p3, `report` p4 |
| Balancing with footnotes and keep groups | `layout/tests.rs: notes_from_both_columns_share_one_area_in_reference_order`; sample `columns` p1 and p2 |
| Footnotes: one full-width area, reading order, on the reference page | `layout/pages.rs: places_notes_in_order_on_the_pages_of_their_references`; `layout/tests.rs: notes_from_both_columns_share_one_area_in_reference_order`; sample `columns` |
| Footnote continuation with marker | `layout/pages.rs: a_long_note_continues_on_the_next_page`, `layout/tests.rs: sets_raised_markers_and_notes_below_the_text_with_a_continuation_marker`. **No sample shows it, see Gaps** |
| Footnotes part of page scoring | `layout/pages.rs: a_note_moves_the_break_before_its_reference` |

## Images and tables

| Requirement | Evidence |
| --- | --- |
| Images in order with captions, moved together | `layout/tests.rs: an_image_and_its_caption_move_together_to_the_next_page`, `an_image_in_columns_has_the_column_width_and_moves_with_its_caption_to_the_next_column` |
| Downscale oversized, never enlarge | `layout/tests.rs: images_shrink_proportionally_to_the_frame_and_never_grow`, `a_tall_image_shrinks_to_fit_below_its_heading` |
| PNG, JPEG, SVG local; others rejected; no remote | `image.rs: decodes_each_format_with_its_size_in_points`, `rejects_gif_and_unknown_extensions`, `rejects_svg_that_refers_to_other_files`; `markdown.rs: reports_images_that_are_not_alone_local_and_untitled` |
| Tables wrap, repeat headers, break between rows | `layout/tests.rs: a_long_table_breaks_between_rows_and_repeats_its_header`, `a_row_is_as_tall_as_its_tallest_cell_with_padding_and_rule`; sample `tables` p1 to p2 |
| A row taller than the page is an error | `layout/tests.rs: reports_a_row_taller_than_the_page_with_its_header` |
| Column spans, rich cells | `layout/table_tests.rs: a_spanning_cell_is_set_across_its_columns`; sample `offer` p2 to p4 |
| Unbreakable content is an error, not clipped | `layout/tests.rs: reports_a_word_wider_than_the_line_at_its_paragraph`, `reports_a_word_too_wide_for_the_narrowest_columns_at_its_cell`; `cli.rs: rejects_code_lines_wider_than_the_text_area`; acceptance (overflow) |

## Document structure, citations

| Requirement | Evidence |
| --- | --- |
| Title block, abstract, title page | `layout/structure_tests.rs: a_title_page_stands_alone_and_body_pages_follow_its_parity`; samples `report`, `report-de` p1 |
| Numbered headings, TOC with final pages | `layout/structure_tests.rs: numbers_headings_to_the_numbering_depth_and_bookmarks_them`, `the_table_of_contents_lists_headings_with_their_final_pages_and_links` |
| Page numbers, running headers | `layout/structure_tests.rs: headers_show_the_section_and_subsection_of_each_page_and_footers_the_page` |
| Cross-references agree with the final layout | `layout/structure_tests.rs: references_show_the_number_and_final_page_of_each_kind`, `settling_repeats_layout_until_shown_pages_agree_and_reports_pages_that_keep_moving`; acceptance (references) |
| BibTeX, author-date and numeric, keys, groups, locators, narrative | `bibliography/tests.rs: formats_author_date_citations_in_english`, `formats_numeric_citations_with_grouping_locators_and_narrative_form`; `bibliography/syntax.rs` tests; samples `report`, `report-de` p9 and p10 |
| Entry types and required fields | `bibliography/tests.rs: reads_every_supported_type_from_the_fixture`, `reports_missing_required_fields_per_entry_in_file_order` |
| Missing keys and fields are errors | `cli.rs: reports_citations_without_a_bibliography_or_entry_at_their_location`, `reports_bibliography_errors_at_their_line_in_the_bib_file` |
| Unknown optional fields ignored | `bibliography/entry.rs: ignores_unsupported_text_in_unused_fields_and_rejects_crossref` |

## Resources, diagnostics, PDF output

| Requirement | Evidence |
| --- | --- |
| Resource origin kept through resolution | `cli.rs: loads_a_document_with_a_partial_theme_and_tracks_resource_origins` |
| Installed fonts by theme; missing is an error | `installed.rs: reports_a_missing_family_or_face_with_what_was_searched_for`; `text/tests.rs: missing_font_file_names_the_property` |
| Diagnostics with source location | `markdown.rs: collects_every_diagnostic_with_its_location`; `cli.rs: reports_image_problems_at_their_reference_with_the_resource` |
| Success versus failure of the CLI | `cli.rs: check_reports_layout_errors_without_writing_a_pdf`; exit codes asserted in every `cli.rs` run |
| Embedded fonts | acceptance (pdf), samples (`pdffonts`: emb, sub, uni all yes) |
| Selectable, searchable text | acceptance (text map per font), samples (`pdftotext` returns the text, 16 PDFs) |
| Clickable links and cross-references | acceptance (references, pdf); `cli.rs: renders_the_report_with_navigation_in_both_themes_the_same_every_time` |
| Heading bookmarks | acceptance (pdf); `pdf.rs: links_to_anchors_and_nests_bookmarks_by_level` |
| Identical layout across platforms | **Missing**: CI `compare` job, never run. Same-machine stability: acceptance (stability), `cli.rs: ..._the_same_every_time` tests |
| Offline on all three platforms | offline (macOS run, network denied by `sandbox-exec`, 13 samples identical); Linux and Windows missing |

## Amendments

| Requirement | Evidence |
| --- | --- |
| Business: installed fonts, weights 100 to 900 | `text/tests.rs: matches_the_nearest_weight_and_then_the_style`; `installed.rs` tests; `cli.rs: renders_variable_weights_and_italics_and_rejects_unavailable_faces` |
| Business: tracking, capitals, custom styles, keep-with-next | `layout/style_tests.rs: sets_a_custom_style_in_capitals_with_letter_spacing`, `a_kept_paragraph_never_ends_a_page_without_the_next_block`; sample `typography` |
| Business: unmirrored margins, narrow prose, anchored slot groups | `layout/structure_tests.rs: margins_mirror_on_even_pages_unless_mirroring_is_off`, `prose_keeps_the_text_width_from_the_inner_edge_and_wide_blocks_span_the_frame`, `title_page_groups_sit_at_their_anchor_and_offset`; sample `offer` |
| Business: list tables, spans, rules | `layout/table_tests.rs`; `markdown.rs: parses_list_tables_with_column_attributes_block_cells_spans_and_styles`; sample `offer` |
| Typesetting: tagged PDF, PDF/UA, PDF/A | `layout/tag_tests.rs`; `cli.rs: tags_every_pdf_and_rejects_an_undescribed_image_under_pdf_ua`; veraPDF **missing** |
| Typesetting: EPUB accessibility metadata | `epub/accessibility.rs: claims_wcag_only_with_described_images_and_legible_text_colors`; epubcheck **missing** |
| Typesetting: smart punctuation | `markdown.rs: sets_dashes_ellipses_and_quotes_of_the_language` |
| Typesetting: syntax highlighting | `highlight.rs` tests; `layout/tests.rs: highlighting_colors_code_without_moving_it`; sample `report` p10 |
| Typesetting: index | `index.rs` tests; `layout/index_tests.rs`; samples `report` p11, `book` p9 |
| Typesetting: margin notes, sidenotes | `layout/margin_tests.rs`; sample `margins` (default and theme) |
| Typesetting: callouts | `layout/callout_tests.rs`; `markdown.rs: parses_github_alerts_as_callouts_and_leaves_other_quotations_alone`; sample `report` p9, p10 |
| Typesetting: top and bottom placement | `layout/margin_tests.rs: a_top_figure_goes_to_the_top_of_the_page_its_anchor_falls_on`, `a_bottom_table_sits_above_the_footnotes`; sample `margins` |
| Desktop: project folders, exports | `cli.rs: project_*`, `exports_*` tests; sample `project` |
| Desktop: comments, inline marks, todos | `markdown/comments.rs` tests; `layout/mark_tests.rs`; `cli.rs: a_final_export_fails_while_todos_remain`; sample `marks` |

## Acceptance section of the briefing

| Requirement | Evidence |
| --- | --- |
| Reviewed English and German samples, custom themes | `samples/`: `en`, `de`, `report`, `report-de` (with `samples/themes/report.json` as well), `book`, `offer`, `typography`, `margins`, `columns`, `pagination`, `images`, `tables`, `marks`, `project`. Inspected 2026-11-03; owner's review missing |
| Corpus: columns 1-2-1, across pages, full-width, mixed footnotes, long footnote continuation, multipage tables | See the rows above. **Long footnote continuation missing from samples** |
| Automated overflow, lost content, references, stability | acceptance |
| Theme behavior: default and one custom theme | `cli.rs` theme tests; samples rendered with `report.json`, `margins.json`, `business.json`, `novel.json`, `typography-styles.json` |
| Page variants, missing slot values, invalid versions, unknown fields | rows under Theme architecture |
| Customization keeps pagination, column, footnote guarantees | acceptance runs on every themed sample; `layout/table_tests.rs`, `layout/style_tests.rs` |
| Visual review of spacing, hyphenation, hierarchy, density, balance, captions, footnotes, bibliography | 2026-11-03 pass in progress; owner's review missing |
| Benchmarks 10, 50, 100 pages | [progress](PROGRESS.md#verification), limits in [decisions](DECISIONS.md#2026-11-03-release-acceptance-limits-desktop-stack-desktop-ci) |

## Phase 2 contract

Every requirement of [the phase 2 contract](RUST_CRATE_API_PHASE_2.md), in its order, with what shows it as of 2026-11-04. Tests are named `file: name`; `renderer`, `cancel`, `diagnostic`, and `source map` mean `src/renderer_tests.rs`, `src/cancel_tests.rs`, `src/diagnostic_tests.rs`, and `src/source_map_tests.rs`. **Missing** means no evidence exists. The API as built is in [the API record](PHASE_2_API.md). The numbers are in [progress](PROGRESS.md#verification).

| Requirement | Evidence |
| --- | --- |
| **Public API surface** | |
| A simple path: Markdown, configuration, and resolution in, PDF and diagnostics out | renderer: `the_simple_and_the_explicit_path_write_the_same_pdf`; `examples/render.rs`; the first root doctest |
| A layout path: an immutable inspectable result, then PDF export without repeating layout | source map: `a_layout_exports_alone_and_equals_a_render`; `examples/inspect.rs`; the second root doctest; parity |
| Typed configuration, themes, resources, diagnostics, cancellation, layout inspection, and export are documented | rustdoc of every public module; CI builds `cargo doc --no-deps` with `-D warnings` |
| In-memory inputs and PDF bytes without temporary files | renderer: `a_memory_resolver_renders_a_report_with_its_theme_and_images_as_files_would`; `examples/in_memory.rs` |
| Parser internals, intermediate structures, scoring, and caches stay private | `cargo doc --no-deps` lists `cancel`, `diagnostic`, `input`, `output`, `resource`, and the root items only. `src/main.rs` is a separate crate and builds from them without the `editor` feature: `grep -nE 'editor\|doc\(hidden\)' src/main.rs` finds nothing |
| A small public surface | The six modules above; `src/editor.rs` is cut to seven items with a reason each, outside `API_VERSION` |
| **Configuration and themes** | |
| Layers apply in order: bundled defaults, one theme, document settings, caller overrides | public API: `explicit_overrides_beat_the_document_the_theme_and_the_defaults_in_that_order`; cli: `later_layers_override_earlier_ones_and_the_theme_flag_beats_front_matter`; `examples/theme.rs` |
| Objects merge by field, arrays replace, omitted values inherit, null needs schema support | cli: `objects_merge_by_field_arrays_replace_and_permitted_nulls_are_kept`, `rejects_null_where_the_type_does_not_allow_it` |
| Document settings cannot redefine template structures; themes cannot disable layout guarantees | `config/front_matter.rs: rejects_null_unknown_keys_and_template_fields`; cli: `rejects_wrong_or_missing_versions_and_unknown_fields` (a theme has no field for either) |
| Resource origins survive resolution: theme assets from the theme origin, document assets from the document's | public API: `a_theme_and_a_document_read_the_same_name_from_their_own_folders` (one name in two folders, each used by its own reference); cli: `named_themes_load_from_the_os_config_directory_with_their_asset_origin`; diagnostic: `a_missing_font_names_the_resource_with_its_origin` |
| The simple and the explicit path resolve identical inputs identically | renderer: `the_simple_and_the_explicit_path_write_the_same_pdf`; parity |
| Configuration errors are reported before layout | renderer: `errors_in_the_configuration_are_structured`; `Job::configure` is the only way to a `Configured`, so a `Layout` cannot exist for an invalid configuration |
| **Resource resolution** | |
| Fonts, images, and bibliography come through a caller-supplied resolver, with file and memory implementations | `resource.rs: a_memory_resolver_reads_by_origin_and_lists_below_a_folder`; renderer: `a_memory_resolver_renders_a_report_...`; parity (file resolver) |
| No implicit filesystem or network access in the core | `grep -rnE 'std::fs\|std::env\|process::exit\|println!' src` outside `src/main.rs`, `src/resource.rs`, tests, and doc comments finds nothing (run 2026-11-04); the dependency list has no network crate; `scripts/check-offline.sh` for the CLI. No automated test: the grep is by hand |
| Bundled fonts and hyphenation need no discovery | renderer: `the_simple_and_the_explicit_path_write_the_same_pdf` renders with an empty `MemoryResolver` |
| Explicit document and theme origins | public API test above; `Origin` is a field of `DocumentText::Single` and `ThemeSelection::Json` |
| Failures name the resource, the origin, and the source location | renderer: `a_resolver_failure_names_the_resource_and_its_origin_and_harms_nothing`; diagnostic: `a_missing_image_has_a_resource_and_a_place_in_the_document` |
| Documented concurrency contract; no assumption that a path means unchanged bytes; reuse only on established content | rustdoc, "Resources and reuse" on the crate root; renderer: `a_replaced_font_is_seen_and_an_unchanged_one_is_not_built_again`, `a_replaced_image_is_seen_and_an_unchanged_one_is_not_decoded_again` |
| **Inspectable layout result** | |
| Pages, drawing content, glyphs with fonts, images with data, links with destinations, source mappings, navigation | `output.rs` types; acceptance (references: anchors, links, and bookmarks all land on a page); source map: `every_text_item_of_the_samples_is_exact_and_round_trips`; `examples/inspect.rs` |
| Retains fonts and images after its input and the renderer are released | source map: `a_layout_exports_alone_and_equals_a_render` (report, images, and typography: the PDF after all else is gone equals a render); renderer: `what_two_jobs_did_not_use_is_forgotten_and_a_layout_keeps_its_own` |
| PDF export repeats no shaping, pagination, or resolution | source map: `exporting_reads_nothing_and_shapes_nothing` (a counting resolver and a shaping counter do not move); `Layout::pdf` takes no resolver |
| Supports faithful native preview drawing | `druck-desktop` paints pages from a `Layout` alone: `cargo test -p druck-desktop` (`raster.rs`: `text_sits_on_its_baseline_and_turned_text_turns_about_its_start`, `variable_fonts_draw_at_the_instance_they_were_shaped_for`, `raster_and_svg_images_fill_their_rect`). The painter in `druck-desktop` is the one client, so a second painter is untested |
| Useful source inspection | source map tests: hyphens, no-break spaces, smart punctuation, comments, projects, footnotes, tables, code; `a_point_on_a_page_finds_its_text_and_a_place_finds_its_rectangle` |
| Coordinates, units, drawing order, glyph interpretation, resources, and mapping are documented | `output.rs` module docs; [the API record](PHASE_2_API.md#5-layout-result) |
| No backend-specific handles | `Face::data`, `index`, and `coordinates` and `Picture::source` and `pixels` are plain bytes and numbers; `usvg::Tree` for SVG is a library type, not a backend handle |
| **Execution, reuse, and concurrency** | |
| Synchronous operations; no async runtime or scheduler | `Cargo.toml` has neither; the crate starts no thread outside tests |
| Cancellation covers resource preparation, layout, and PDF export with clear outcomes | cancel: `the_resource_check_point_stops_before_the_next_read`, `the_image_check_point_stops_before_the_next_decode`, `the_block_check_point_stops_before_the_next_block`, `the_pagination_check_point_stops_the_search_every_64_lines`, `the_page_check_point_stops_an_export_with_no_bytes`, `the_chapter_check_point_stops_an_epub_with_no_bytes`; `examples/cancel.rs` |
| Cancellation never returns incomplete output as success | cancel: `the_page_check_point_stops_an_export_with_no_bytes`, `a_cancel_while_finishing_a_pdf_returns_no_bytes`, `a_cancelled_flow_with_nested_blocks_stops_cleanly_at_every_block` |
| A cancelled or failed job leaves the renderer and other jobs alone | cancel: `cancelled_jobs_keep_what_the_renderer_holds`; renderer: `a_resolver_failure_names_the_resource_and_its_origin_and_harms_nothing` |
| A reusable renderer keeps font and resource preparation and takes separate requests | renderer: `a_replaced_font_...`, `a_replaced_image_...`; `examples/concurrent.rs` |
| Independent concurrent jobs; no global mutable configuration or renderer | renderer: `concurrent_jobs_with_different_themes_give_what_they_give_alone`; `examples/concurrent.rs` (four documents and four themes, each equal to its single run). The only statics in `src` are a hash key, a syntax set, and an SVG font database, each set once and then read |
| Sharing is documented and enforced by types | `src/renderer.rs` assertions: `Renderer`, `Layout`, `Input`, `Cancel`, and both resolvers are `Send + Sync`, a job is `Send` and fails to compile if it becomes `Sync`; rustdoc "Threads" |
| Reuse keeps the reproducibility contract and respects changed contents and configuration | parity (every route writes the same bytes, with and without a source map); acceptance (stability); renderer: the replaced font and image tests |
| **Diagnostics and failure** | |
| Structured diagnostics with locations, properties, resources, and origins | diagnostic: `an_error_in_a_project_names_the_chapter_file_and_its_line`, `a_bad_theme_value_names_the_theme_and_the_property`, `a_missing_font_names_the_resource_with_its_origin`, `a_bad_override_names_the_overrides` |
| Success, warnings, fatal errors, and cancellation are distinct | `Failure` and `Rendered::warnings`; diagnostic: `warnings_sit_beside_success`; cancel tests |
| Phase 1 behavior for unsupported syntax, invalid configuration, missing citations and resources, constraints, and overflow | cli: `reports_unsupported_content_with_its_location_and_writes_nothing`, `reports_citations_without_a_bibliography_or_entry_at_their_location`, `reports_a_missing_theme_font_relative_to_the_theme`, `check_reports_layout_errors_without_writing_a_pdf`, `rejects_code_lines_wider_than_the_text_area`; a matrix of 87 CLI commands gave the same stdout, stderr, and exit code before and after step 5 |
| Stable meanings for public categories | rustdoc of `Severity`, `Source`, `Diagnostic`, and `Failure` |
| The crate does not exit or print | the grep above. No automated test |
| **CLI parity** | |
| The CLI uses the public API only | `src/main.rs` is a binary crate: it cannot see `pub(crate)` items, and it builds without `editor` and `doc(hidden)` items. Its tests are all of `tests/cli.rs` |
| Identical layout for the CLI and an embedded client, direct and layout-then-export | `tests/parity.rs`: every sample, each with its theme, the sample project and its `print` export, and the book when `DRUCK_BOOK` names a project, through `Renderer::render`, through configure, layout, and `Layout::pdf`, and through the binary: all write the same bytes, which is stricter than the same layout. EPUBs too. Run on macOS only |
| **Compatibility and documentation** | |
| The API and the schema are versioned separately; supported schema versions are documented; unsupported ones fail clearly | `API_VERSION` and `THEME_SCHEMA_VERSIONS`; public API: `only_the_listed_theme_schema_versions_are_read_and_others_fail_at_the_version`; cli: `rejects_wrong_or_missing_versions_and_unknown_fields`; rustdoc "Versions" and the README |
| Supported interfaces are public and details are private | the module table above; `src/editor.rs` is the one exception, outside the version, and says so |
| Documentation and compilable examples for the six uses | `examples/render.rs`, `theme.rs`, `in_memory.rs`, `inspect.rs`, `cancel.rs`, `concurrent.rs`, run by the `Examples` step of CI; two doctests on the crate root, run by `cargo test` |
| Usable by a desktop client and a backend service, not by running the CLI | `druck-desktop` is a library client; `examples/concurrent.rs` and `in_memory.rs` are service-shaped |
| **Acceptance** | |
| The phase 1 corpus through the public API | acceptance (every sample and theme, public `Layout` only); parity |
| Fully in-memory rendering | renderer: `a_memory_resolver_renders_a_report_...` |
| Filesystem resolution with distinct document and theme origins | public API: `a_theme_and_a_document_read_the_same_name_from_their_own_folders` |
| Structured errors | diagnostic tests |
| Required and optional slots | `layout/structure_tests.rs: a_required_title_slot_without_a_value_is_an_error`, `a_title_block_omits_empty_optional_slots_with_their_spacing_and_keeps_metadata_as_text`, `a_missing_meta_value_omits_its_slot_unless_it_is_required`; cli: `check_reports_a_required_title_slot_without_a_value` |
| Override precedence | the rows under Configuration and themes |
| Strict schema validation | cli and public API tests above |
| A native preview can draw from the display list | the desktop rows above |
| Concurrent jobs with different documents and themes, without leakage | renderer: `concurrent_jobs_with_different_themes_give_what_they_give_alone` |
| Cancellation during meaningful work, then reuse | cancel: `a_cancel_from_another_thread_stops_a_large_layout_promptly` (a 400 to 2,000 paragraph layout cancelled from another thread, then the same renderer writes the same PDF as a new one) |
| Changed resource contents never reuse stale preparation | renderer: `a_replaced_font_...`, `a_replaced_image_...`, `the_extension_still_decides_when_the_content_is_already_decoded` |
| Targeted API tests and examples on supported native platforms | macOS: run 2026-11-04. **Missing:** Linux and Windows, since no workflow has run |
| Phase 1 limits revisited with a reused renderer and concurrent work | [progress](PROGRESS.md#verification) and [decisions](DECISIONS.md#2026-11-04-phase-2-step-5-choices-beyond-the-design) |
| Concurrency limits set from measurement | the same entry: no limit is enforced, because the crate has no scheduler; the guidance is in the rustdoc and the entry |

Pass counts, from a temporary instrumented build in phase 1: citations and the bibliography add no layout pass, and documents with a table of contents settle in two.
