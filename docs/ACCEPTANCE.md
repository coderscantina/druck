# Acceptance evidence

Every requirement of [the briefing](BRIEFING.md) and its amendments, with what shows it. Evidence is a test (`file: name`), a sample with the page inspected, or a command. **Missing** means no evidence exists yet. Nothing here narrows the briefing. The state of the release gate is in [progress](PROGRESS.md).

Test files: `src/<module>` unit tests, `tests/cli.rs` (CLI), `src/acceptance_tests.rs` (every sample, see below). Visual evidence is from the 2026-11-03 pass in [progress](PROGRESS.md#verification), which does not replace the owner's review.

## Gaps

- **Linux and Windows:** layout identity across platforms, `cargo test`, and the sample renders were never run. CI covers them, but no workflow has run because nothing is pushed.
- **Owner's visual review:** not done.
- **Continuing footnote in the corpus:** no sample shows one. `samples/pagination.md` says note 3 continues, but it fits on page 2. `layout/pages.rs: a_long_note_continues_on_the_next_page` covers the behavior.
- **Conformance validators:** veraPDF (PDF/A, PDF/UA), epubcheck, Ace, and screen readers were never run.
- **Performance limits are not enforced** by a test or CI.
- **Windows offline run:** `scripts/check-offline.sh` has no Windows branch.
- **Desktop notices:** 41 crates have no license file, one is MPL-2.0.

## Test files by name

| Short | Means |
| --- | --- |
| acceptance | `src/acceptance_tests.rs: every_sample_passes_the_acceptance_checks`, run over every sample and each sample's theme: overflow (nothing drawn outside its page), lost content (every long word of the prose is on a page), references (every link, anchor, and bookmark has a placed destination, no `??`), stability (two renders write the same PDF), PDF features (fonts embedded with a text map, link annotations equal links, outline present) |
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

## Boundaries phase 2 will expose

From the code as it stands, not from the [phase 2 contract](RUST_CRATE_API_PHASE_2.md). All are `pub` in a library no one depends on yet, so each can still change.

| Boundary | Where | Shape today | Needs before it is public |
| --- | --- | --- | --- |
| Config resolution | `config::resolve::resolve(Inputs) -> Result<Config, Vec<Diagnostic>>`; `pipeline::configure` | `Inputs` holds an optional theme value with its `Source`, front matter settings with a `Source`, and CLI overrides. `Config` is the resolved, typed result | A caller-facing builder instead of `Source` paths, and overrides that are not `--set` strings. `resolve_settings` is `pub(crate)` and reads theme files itself |
| Resource access | `config::source::{Source, Origin, Resource}`; `pipeline::load_images`, `load_bibliography`, `find_installed`, `text::Fonts::load` | Every read is `std::fs` on a path, in `pipeline`, `epub`, and font loading. Font bytes read from disk are leaked once per render | A resolver trait for theme assets, images, bibliography, fonts, with origins kept. Fonts must be owned, not leaked |
| Layout result | `layout::layout(...) -> Result<page::Output, Vec<Diagnostic>>` | `page::Output`: pages of positioned `Item`s, anchors, outline, page numbers, tag tree, asides, warnings. Eight inputs are passed separately (`#[allow(too_many_arguments)]`) | One input value. `Output` is enough for inspection, but its types (`ShapedRun`, `Glyph`, `Tree`) expose internals. No source mapping from a page position back to Markdown (needed by the desktop, see [decisions](DECISIONS.md#2026-11-03-desktop-spike-results)) |
| PDF export | `pdf::write(&Output, &Fonts, &Metadata, &Settings) -> Result<Vec<u8>, Vec<Diagnostic>>`; `pipeline::write_pdf` | Pure function from layout to bytes, no files. EPUB (`epub`) is a second exporter over the document model, not over `Output` | None beyond naming, and a streaming or cancellable form |
| Diagnostics | `diagnostic::Diagnostic` | `source`, 1-based `location`, `property`, `message`, `warning`, plus a sort position. Errors and warnings share one type | Stable, serializable kinds; formatting is done by `Display`, with paths made relative through process-wide state (`show_relative_to`) |
| Cancellation, concurrency | none | `pipeline::configure` and `typeset_with` have no process effects and run from any thread (the desktop does). `pipeline::load` sets a global path display | A cancellation token checked in the settling loop and page search |
| Phase 2 stays deferred | | No crate is published, `docs/RUST_CRATE_API_PHASE_2.md` is a contract only | |
