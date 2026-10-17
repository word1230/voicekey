<!-- 无 props 的纯图形组件。这个空的 script 块是给 svelte-check 的：没有它，
     svelte2tsx 不为本组件生成 .d.ts，import 它的地方报「implicitly has an 'any' type」。 -->
<script lang="ts"></script>

<svg viewBox="0 0 64 64" xmlns="http://www.w3.org/2000/svg">
  <defs>
    <linearGradient id="vkwg" x1="0" x2="1">
      <stop offset="0" stop-color="#22c3ff" /><stop offset=".55" stop-color="#7b5cff" /><stop offset="1" stop-color="#ff3d8b" />
    </linearGradient>
  </defs>
  <path d="M6 32c8 0 10-1 14 8s6 10 9 0 4-18 9-18 6 12 10 12 6-2 10-2" fill="none" stroke="url(#vkwg)" stroke-width="5" stroke-linecap="round" />
</svg>
