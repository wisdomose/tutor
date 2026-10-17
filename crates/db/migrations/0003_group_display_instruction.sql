-- Questions in a group are always shown together in the student app, which numbers
-- them itself. The printed instruction ("Study it and answer questions 31 to 33")
-- stays for reviewers; students get display_instruction, without those numbers.
alter table question_groups add column display_instruction text;

-- Existing groups: drop the sentence that names question numbers.
update question_groups
set display_instruction = nullif(btrim(regexp_replace(
      instruction, '[^.]*\mquestions?\M\s*[0-9][^.]*\.?', '', 'gi')), '')
where instruction is not null;

-- New check reason: refers_to_number (the question's own text mentions another
-- question by number, e.g. "using your answer to question 31").

-- Inline options were saved with the full stop that only separates them in print
-- ("stolon."). Drop a single trailing one from single-sentence options.
update options set body = rtrim(body, '.')
where body ~ '^[^.]+\.$' and body !~* 'etc\.$';
